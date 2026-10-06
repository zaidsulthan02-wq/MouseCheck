//! Raw Input capture engine.
//!
//! A dedicated, time-critical thread owns a message-only window that is registered
//! for mouse Raw Input. Every HID report from every mouse arrives as one WM_INPUT
//! message; we stamp it with QueryPerformanceCounter the moment it is dequeued and
//! push it into a channel. A separate collector thread drains the channel into the
//! shared store, so the UI can never block the capture thread with a lock.
//!
//! Important Windows behaviour (Win11, build 25272+): raw input delivered to processes
//! that are NOT in the foreground is throttled and coalesced. Our measurements are
//! therefore only valid while MouseCheck owns the foreground window. Every event is
//! tagged with that state so the analysis can reject background data.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use windows_sys::Win32::Foundation::{FILETIME, HANDLE, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcessId, GetCurrentThread, GetSystemTimes, SetThreadPriority, THREAD_PRIORITY_TIME_CRITICAL,
};
use windows_sys::Win32::UI::Input::{
    GetRawInputData, GetRawInputDeviceInfoW, GetRawInputDeviceList, GetRegisteredRawInputDevices, HRAWINPUT, RAWINPUT,
    RAWINPUTDEVICE, RAWINPUTDEVICELIST, RAWINPUTHEADER, RID_INPUT, RIDEV_DEVNOTIFY, RIDEV_INPUTSINK, RIDI_DEVICENAME,
    RIM_TYPEMOUSE, RegisterRawInputDevices,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId,
    HWND_MESSAGE, MSG, RegisterClassExW, SetTimer, TranslateMessage, WNDCLASSEXW,
};

const WM_INPUT: u32 = 0x00FF;
const WM_TIMER: u32 = 0x0113;

/// How much recent history the store keeps for the live view.
const RECENT_SECONDS: f64 = 6.0;

/// Raw button flag bits (usButtonFlags).
pub mod btn {
    pub const WHEEL: u16 = 0x0400;
    /// (down, up) flag pairs for Left, Right, Middle, Back (X1), Forward (X2).
    pub const PAIRS: [(u16, u16); 5] =
        [(0x0001, 0x0002), (0x0004, 0x0008), (0x0010, 0x0020), (0x0040, 0x0080), (0x0100, 0x0200)];
    pub const NAMES: [&str; 5] = ["Left", "Right", "Middle", "Back (M4)", "Forward (M5)"];
}

/// One raw mouse report.
#[derive(Clone, Copy, Debug, Default)]
pub struct Ev {
    /// QueryPerformanceCounter ticks at dequeue time.
    pub t: i64,
    /// Index into `Store::devices`.
    pub dev: u32,
    pub dx: i32,
    pub dy: i32,
    pub bflags: u16,
    pub bdata: i16,
    /// MouseCheck owned the foreground window when this arrived (full-rate delivery).
    pub fg: bool,
    /// Absolute-coordinate device (tablet / remote desktop). Not a real mouse sensor.
    pub abs: bool,
}

impl Ev {
    pub fn moved(&self) -> bool {
        self.dx != 0 || self.dy != 0
    }
}

#[derive(Clone, Debug, Default)]
pub struct DeviceInfo {
    pub handle: usize,
    pub path: String,
    pub vid: u16,
    pub pid: u16,
    pub product: String,
    pub manufacturer: String,
    pub resolved: bool,
    pub events: u64,
    pub motion_events: u64,
}

impl DeviceInfo {
    pub fn display_name(&self) -> String {
        if self.handle == 0 {
            return "Injected / virtual input".into();
        }
        let base = if !self.product.is_empty() {
            self.product.clone()
        } else if self.path.to_ascii_uppercase().contains("RDP_MOU") {
            "Remote desktop mouse".into()
        } else {
            "Unknown mouse".into()
        };
        if self.vid != 0 { format!("{base} ({:04X}:{:04X})", self.vid, self.pid) } else { base }
    }
}

#[derive(Default)]
pub struct Store {
    pub devices: Vec<DeviceInfo>,
    pub recent: VecDeque<Ev>,
    pub recording: Option<Vec<Ev>>,
    /// (qpc ticks, total CPU usage 0..100)
    pub cpu: VecDeque<(i64, f32)>,
    pub recording_cpu: Vec<f32>,
}

pub struct Shared {
    pub store: Mutex<Store>,
    pub freq: i64,
    pub foreground: AtomicBool,
    pub registered: AtomicBool,
    /// How many times another component stole our raw-input registration and we took it back.
    pub reg_restores: AtomicU32,
    pub hwnd: AtomicUsize,
    /// Devices seen by the capture thread (handle -> index); only touched by capture thread
    /// and the startup enumeration (before the thread starts).
    tx: Mutex<Option<Sender<Ev>>>,
}

impl Shared {
    pub fn now(&self) -> i64 {
        qpc()
    }

    pub fn ticks_to_us(&self, ticks: i64) -> f64 {
        ticks as f64 * 1_000_000.0 / self.freq as f64
    }

    pub fn start_recording(&self) {
        let mut s = self.store.lock().unwrap();
        s.recording = Some(Vec::with_capacity(400_000));
        s.recording_cpu.clear();
    }

    pub fn stop_recording(&self) -> (Vec<Ev>, Vec<f32>) {
        let mut s = self.store.lock().unwrap();
        let ev = s.recording.take().unwrap_or_default();
        let cpu = std::mem::take(&mut s.recording_cpu);
        (ev, cpu)
    }
}

static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();

struct ThreadState {
    tx: Sender<Ev>,
    lookup: HashMap<usize, u32>,
    timer_ticks: u64,
    cpu_prev: Option<(u64, u64, u64)>,
}

thread_local! {
    static THREAD: std::cell::RefCell<Option<ThreadState>> = const { std::cell::RefCell::new(None) };
}

pub fn qpc() -> i64 {
    let mut t = 0i64;
    unsafe { QueryPerformanceCounter(&mut t) };
    t
}

/// Starts the capture + collector threads. Call once, after the UI window exists.
pub fn start() -> Arc<Shared> {
    let mut freq = 0i64;
    unsafe { QueryPerformanceFrequency(&mut freq) };
    let (tx, rx) = mpsc::channel::<Ev>();
    let shared = Arc::new(Shared {
        store: Mutex::new(Store::default()),
        freq,
        foreground: AtomicBool::new(false),
        registered: AtomicBool::new(false),
        reg_restores: AtomicU32::new(0),
        hwnd: AtomicUsize::new(0),
        tx: Mutex::new(Some(tx)),
    });
    let _ = SHARED.set(shared.clone());

    // Pre-populate the device list so mice show up before they move.
    let mut lookup = HashMap::new();
    {
        let mut store = shared.store.lock().unwrap();
        for (handle, path) in enumerate_mice() {
            lookup.insert(handle, store.devices.len() as u32);
            store.devices.push(DeviceInfo { handle, path, ..Default::default() });
        }
    }

    let tx = shared.tx.lock().unwrap().take().unwrap();
    std::thread::Builder::new()
        .name("raw-input-capture".into())
        .spawn(move || capture_thread(tx, lookup))
        .expect("spawn capture thread");

    let s2 = shared.clone();
    std::thread::Builder::new()
        .name("raw-input-collector".into())
        .spawn(move || collector_thread(s2, rx))
        .expect("spawn collector thread");

    shared
}

fn collector_thread(shared: Arc<Shared>, rx: Receiver<Ev>) {
    let keep = (RECENT_SECONDS * shared.freq as f64) as i64;
    let mut batch: Vec<Ev> = Vec::with_capacity(4096);
    loop {
        match rx.recv_timeout(Duration::from_millis(5)) {
            Ok(ev) => batch.push(ev),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        while let Ok(ev) = rx.try_recv() {
            batch.push(ev);
            if batch.len() > 65_536 {
                break;
            }
        }
        if batch.is_empty() {
            continue;
        }
        let mut s = shared.store.lock().unwrap();
        for ev in &batch {
            if let Some(d) = s.devices.get_mut(ev.dev as usize) {
                d.events += 1;
                if ev.moved() {
                    d.motion_events += 1;
                }
            }
        }
        if let Some(rec) = s.recording.as_mut() {
            rec.extend_from_slice(&batch);
        }
        s.recent.extend(batch.drain(..));
        if let Some(last) = s.recent.back().map(|e| e.t) {
            while s.recent.front().is_some_and(|e| e.t < last - keep) {
                s.recent.pop_front();
            }
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn capture_thread(tx: Sender<Ev>, lookup: HashMap<usize, u32>) {
    unsafe {
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
        THREAD.with(|t| *t.borrow_mut() = Some(ThreadState { tx, lookup, timer_ticks: 0, cpu_prev: None }));

        let hinst = GetModuleHandleW(std::ptr::null());
        let class = wide("MouseCheckRawInputSink");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: hinst,
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassExW(&wc);
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            class.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            std::ptr::null_mut(),
            hinst,
            std::ptr::null(),
        );
        if let Some(sh) = SHARED.get() {
            sh.hwnd.store(hwnd as usize, Ordering::Relaxed);
        }
        register(hwnd);
        update_foreground();
        SetTimer(hwnd, 1, 100, None);

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

unsafe fn register(hwnd: HWND) -> bool {
    let dev = RAWINPUTDEVICE {
        usUsagePage: 0x01,
        usUsage: 0x02,
        dwFlags: RIDEV_INPUTSINK | RIDEV_DEVNOTIFY,
        hwndTarget: hwnd,
    };
    let ok = unsafe { RegisterRawInputDevices(&dev, 1, std::mem::size_of::<RAWINPUTDEVICE>() as u32) != 0 };
    if let Some(sh) = SHARED.get() {
        sh.registered.store(ok, Ordering::Relaxed);
    }
    ok
}

/// True if our window is still the raw-input target for mice in this process.
unsafe fn still_registered(hwnd: HWND) -> bool {
    let mut devs: [RAWINPUTDEVICE; 16] = unsafe { std::mem::zeroed() };
    let mut n = devs.len() as u32;
    let got = unsafe {
        GetRegisteredRawInputDevices(devs.as_mut_ptr(), &mut n, std::mem::size_of::<RAWINPUTDEVICE>() as u32)
    };
    if got == u32::MAX {
        return false;
    }
    devs[..got as usize].iter().any(|d| d.usUsagePage == 1 && d.usUsage == 2 && d.hwndTarget == hwnd)
}

fn update_foreground() {
    unsafe {
        let fg = GetForegroundWindow();
        let mut pid = 0u32;
        if !fg.is_null() {
            GetWindowThreadProcessId(fg, &mut pid);
        }
        if let Some(sh) = SHARED.get() {
            sh.foreground.store(pid == GetCurrentProcessId(), Ordering::Relaxed);
        }
    }
}

fn ft(f: FILETIME) -> u64 {
    ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_INPUT => {
            // Timestamp first, before anything else can delay us.
            let t = qpc();
            unsafe { on_input(t, lp as HRAWINPUT) };
            unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
        }
        WM_TIMER => {
            on_timer(hwnd);
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

fn on_timer(hwnd: HWND) {
    update_foreground();
    let Some(sh) = SHARED.get() else { return };
    THREAD.with(|t| {
        let mut t = t.borrow_mut();
        let Some(st) = t.as_mut() else { return };
        st.timer_ticks += 1;

        // Another component (e.g. the UI toolkit) may re-register raw input and steal it.
        unsafe {
            if !still_registered(hwnd) {
                if register(hwnd) {
                    sh.reg_restores.fetch_add(1, Ordering::Relaxed);
                }
            }
        }

        // Total CPU usage since the last tick.
        let (mut idle, mut kernel, mut user) = unsafe { std::mem::zeroed() };
        if unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) } != 0 {
            let cur = (ft(idle), ft(kernel), ft(user));
            if let Some(prev) = st.cpu_prev {
                let di = cur.0.saturating_sub(prev.0) as f64;
                let dk = cur.1.saturating_sub(prev.1) as f64;
                let du = cur.2.saturating_sub(prev.2) as f64;
                let total = dk + du; // kernel time includes idle time
                if total > 0.0 {
                    let usage = ((1.0 - di / total) * 100.0).clamp(0.0, 100.0) as f32;
                    let now = qpc();
                    let mut s = sh.store.lock().unwrap();
                    s.cpu.push_back((now, usage));
                    while s.cpu.len() > 120 {
                        s.cpu.pop_front();
                    }
                    if s.recording.is_some() {
                        s.recording_cpu.push(usage);
                    }
                }
            }
            st.cpu_prev = Some(cur);
        }
    });
}

unsafe fn on_input(t: i64, h: HRAWINPUT) {
    let mut raw: RAWINPUT = unsafe { std::mem::zeroed() };
    let mut size = std::mem::size_of::<RAWINPUT>() as u32;
    let r = unsafe {
        GetRawInputData(
            h,
            RID_INPUT,
            &mut raw as *mut _ as *mut core::ffi::c_void,
            &mut size,
            std::mem::size_of::<RAWINPUTHEADER>() as u32,
        )
    };
    if r == u32::MAX || raw.header.dwType != RIM_TYPEMOUSE {
        return;
    }
    let m = unsafe { raw.data.mouse };
    let (bflags, bdata) = unsafe { (m.Anonymous.Anonymous.usButtonFlags, m.Anonymous.Anonymous.usButtonData) };
    let handle = raw.header.hDevice as usize;
    let fg = SHARED.get().map(|s| s.foreground.load(Ordering::Relaxed)).unwrap_or(false);

    THREAD.with(|tcell| {
        let mut tb = tcell.borrow_mut();
        let Some(st) = tb.as_mut() else { return };
        let dev = match st.lookup.get(&handle) {
            Some(&i) => i,
            None => {
                let path = if handle == 0 { String::new() } else { device_path(raw.header.hDevice) };
                let Some(sh) = SHARED.get() else { return };
                let mut s = sh.store.lock().unwrap();
                let i = s.devices.len() as u32;
                s.devices.push(DeviceInfo { handle, path, resolved: handle == 0, ..Default::default() });
                st.lookup.insert(handle, i);
                i
            }
        };
        let _ = st.tx.send(Ev {
            t,
            dev,
            dx: m.lLastX,
            dy: m.lLastY,
            bflags,
            bdata: bdata as i16,
            fg,
            abs: m.usFlags & 0x01 != 0,
        });
    });
}

fn device_path(h: HANDLE) -> String {
    unsafe {
        let mut len = 0u32;
        GetRawInputDeviceInfoW(h, RIDI_DEVICENAME, std::ptr::null_mut(), &mut len);
        if len == 0 {
            return String::new();
        }
        let mut buf = vec![0u16; len as usize + 1];
        let got = GetRawInputDeviceInfoW(h, RIDI_DEVICENAME, buf.as_mut_ptr().cast(), &mut len);
        if got == u32::MAX {
            return String::new();
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..end])
    }
}

fn enumerate_mice() -> Vec<(usize, String)> {
    unsafe {
        let mut n = 0u32;
        let sz = std::mem::size_of::<RAWINPUTDEVICELIST>() as u32;
        GetRawInputDeviceList(std::ptr::null_mut(), &mut n, sz);
        if n == 0 {
            return vec![];
        }
        let mut list: Vec<RAWINPUTDEVICELIST> = vec![std::mem::zeroed(); n as usize];
        let got = GetRawInputDeviceList(list.as_mut_ptr(), &mut n, sz);
        if got == u32::MAX {
            return vec![];
        }
        list[..got as usize]
            .iter()
            .filter(|d| d.dwType == RIM_TYPEMOUSE)
            .map(|d| (d.hDevice as usize, device_path(d.hDevice)))
            .collect()
    }
}
