//! HID helpers: product names, interface inventory and the *standard* battery reports.
//!
//! Honesty note: most 2.4 GHz gaming mice report battery level only through a
//! vendor-specific protocol on a "vendor-defined" HID collection (that's what the
//! brand's software / web driver talks to). Windows has no generic way to read it.
//! We look for the two standard HID battery usages; if the mouse doesn't expose
//! them we say so instead of guessing.

use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    CM_GET_DEVICE_INTERFACE_LIST_PRESENT, CM_Get_Device_Interface_List_SizeW, CM_Get_Device_Interface_ListW,
};
use windows_sys::Win32::Devices::HumanInterfaceDevice::{
    HIDP_CAPS, HIDP_REPORT_TYPE, HIDP_STATUS_SUCCESS, HIDP_VALUE_CAPS, HidD_FreePreparsedData, HidD_GetFeature,
    HidD_GetHidGuid, HidD_GetInputReport, HidD_GetManufacturerString, HidD_GetPreparsedData, HidD_GetProductString,
    HidP_Feature, HidP_GetCaps, HidP_GetUsageValue, HidP_GetValueCaps, HidP_Input, PHIDP_PREPARSED_DATA,
};
use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};

use crate::capture::DeviceInfo;

/// Parses `VID_xxxx` / `PID_xxxx` out of a device path.
pub fn parse_vid_pid(path: &str) -> (u16, u16) {
    let up = path.to_ascii_uppercase();
    let grab = |key: &str| -> u16 {
        up.find(key)
            .and_then(|i| up.get(i + key.len()..i + key.len() + 4))
            .and_then(|h| u16::from_str_radix(h, 16).ok())
            .unwrap_or(0)
    };
    (grab("VID_"), grab("PID_"))
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

fn open(path: &str, access: u32) -> Option<Handle> {
    let w = wide(path);
    let h = unsafe {
        CreateFileW(
            w.as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };
    if h == INVALID_HANDLE_VALUE || h.is_null() { None } else { Some(Handle(h)) }
}

fn hid_string(h: &Handle, f: unsafe extern "system" fn(HANDLE, *mut core::ffi::c_void, u32) -> bool) -> String {
    let mut buf = [0u16; 256];
    let ok = unsafe { f(h.0, buf.as_mut_ptr().cast(), (buf.len() * 2) as u32) };
    if !ok {
        return String::new();
    }
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end]).trim().to_string()
}

/// Fills vid/pid/product/manufacturer for a device discovered by raw input.
pub fn resolve(dev: &mut DeviceInfo) {
    dev.resolved = true;
    if dev.path.is_empty() {
        return;
    }
    let (vid, pid) = parse_vid_pid(&dev.path);
    dev.vid = vid;
    dev.pid = pid;
    if let Some(h) = open(&dev.path, 0) {
        dev.product = hid_string(&h, HidD_GetProductString);
        dev.manufacturer = hid_string(&h, HidD_GetManufacturerString);
    }
}

#[derive(Clone, Debug)]
pub struct Collection {
    pub path: String,
    pub usage_page: u16,
    pub usage: u16,
    pub kind: String,
    pub product: String,
}

#[derive(Clone, Debug)]
pub struct BatteryReading {
    pub percent: Option<f64>,
    pub raw: u32,
    pub source: String,
}

#[derive(Clone, Debug, Default)]
pub struct HidInventory {
    pub collections: Vec<Collection>,
    pub battery: Option<BatteryReading>,
    pub battery_usage_found: bool,
    pub notes: Vec<String>,
}

fn describe_usage(page: u16, usage: u16) -> String {
    match (page, usage) {
        (0x01, 0x02) => "Mouse (standard)".into(),
        (0x01, 0x06) => "Keyboard (standard; used for macros)".into(),
        (0x01, 0x80) => "System control (power/sleep keys)".into(),
        (0x0C, 0x01) => "Consumer control (media keys)".into(),
        (0x06, _) => "Generic device controls (may carry battery)".into(),
        (0x84, _) | (0x85, _) => "Power / battery device".into(),
        (p, _) if p >= 0xFF00 => format!("Vendor-defined 0x{p:04X} (brand software / driver protocol)"),
        (p, u) => format!("Usage page 0x{p:02X}, usage 0x{u:02X}"),
    }
}

/// Lists every HID collection that belongs to the given VID/PID (the receiver), and tries
/// to read a standard battery value from them.
pub fn inventory(vid: u16, pid: u16) -> HidInventory {
    let mut inv = HidInventory::default();
    if vid == 0 {
        inv.notes.push("This device has no USB vendor/product id (virtual or remote device).".into());
        return inv;
    }
    let needle = format!("VID_{vid:04X}&PID_{pid:04X}");
    for path in hid_interfaces() {
        if !path.to_ascii_uppercase().contains(&needle) {
            continue;
        }
        let Some(h) = open(&path, 0) else { continue };
        let product = hid_string(&h, HidD_GetProductString);
        let mut pre: PHIDP_PREPARSED_DATA = 0;
        if !unsafe { HidD_GetPreparsedData(h.0, &mut pre) } {
            continue;
        }
        let mut caps: HIDP_CAPS = Default::default();
        if unsafe { HidP_GetCaps(pre, &mut caps) } == HIDP_STATUS_SUCCESS {
            inv.collections.push(Collection {
                path: path.clone(),
                usage_page: caps.UsagePage,
                usage: caps.Usage,
                kind: describe_usage(caps.UsagePage, caps.Usage),
                product,
            });
            if inv.battery.is_none() {
                if let Some((found, reading)) = try_battery(&path, pre, &caps) {
                    inv.battery_usage_found |= found;
                    inv.battery = reading;
                }
            }
        }
        unsafe { HidD_FreePreparsedData(pre) };
    }
    if inv.collections.is_empty() {
        inv.notes.push("No HID collections found for this receiver.".into());
    }
    let has_vendor = inv.collections.iter().any(|c| c.usage_page >= 0xFF00);
    if inv.battery.is_none() {
        if inv.battery_usage_found {
            inv.notes.push("A standard battery field exists but could not be read (the mouse may be asleep — move it and refresh).".into());
        } else if has_vendor {
            inv.notes.push("No standard HID battery report. The receiver has a vendor-defined collection, so the battery level is almost certainly only readable through the brand's own protocol. Tell us your exact mouse model so a protocol module can be added.".into());
        } else {
            inv.notes.push("No standard HID battery report on this device.".into());
        }
    }
    inv
}

/// Looks for Generic Device Controls / Battery Strength (0x06/0x20) or Battery System
/// state-of-charge usages (0x85/0x64..0x66). Returns (usage present, reading).
fn try_battery(path: &str, pre: PHIDP_PREPARSED_DATA, caps: &HIDP_CAPS) -> Option<(bool, Option<BatteryReading>)> {
    const WANT: [(u16, u16, &str); 4] = [
        (0x06, 0x20, "HID Battery Strength"),
        (0x85, 0x64, "HID Relative State of Charge"),
        (0x85, 0x65, "HID Absolute State of Charge"),
        (0x85, 0x66, "HID Remaining Capacity"),
    ];
    let mut found = false;
    for (rtype, count, len) in [
        (HidP_Feature, caps.NumberFeatureValueCaps, caps.FeatureReportByteLength),
        (HidP_Input, caps.NumberInputValueCaps, caps.InputReportByteLength),
    ] {
        if count == 0 {
            continue;
        }
        let mut vc: Vec<HIDP_VALUE_CAPS> = vec![Default::default(); count as usize];
        let mut n = count;
        if unsafe { HidP_GetValueCaps(rtype, vc.as_mut_ptr(), &mut n, pre) } != HIDP_STATUS_SUCCESS {
            continue;
        }
        for v in &vc[..n as usize] {
            let usage =
                if v.IsRange { unsafe { v.Anonymous.Range.UsageMin } } else { unsafe { v.Anonymous.NotRange.Usage } };
            let Some(&(_, _, name)) = WANT.iter().find(|w| w.0 == v.UsagePage && w.1 == usage) else { continue };
            found = true;
            if let Some(raw) = read_value(path, rtype, v.ReportID, v.UsagePage, usage, pre, len) {
                let pct = if v.LogicalMax > v.LogicalMin {
                    Some(
                        ((raw as f64 - v.LogicalMin as f64) / (v.LogicalMax - v.LogicalMin) as f64 * 100.0)
                            .clamp(0.0, 100.0),
                    )
                } else {
                    None
                };
                return Some((true, Some(BatteryReading { percent: pct, raw, source: name.into() })));
            }
        }
    }
    Some((found, None))
}

fn read_value(
    path: &str,
    rtype: HIDP_REPORT_TYPE,
    report_id: u8,
    page: u16,
    usage: u16,
    pre: PHIDP_PREPARSED_DATA,
    len: u16,
) -> Option<u32> {
    if len == 0 {
        return None;
    }
    let h = open(path, GENERIC_READ | GENERIC_WRITE).or_else(|| open(path, 0))?;
    let mut buf = vec![0u8; len as usize];
    buf[0] = report_id;
    let ok = unsafe {
        if rtype == HidP_Feature {
            HidD_GetFeature(h.0, buf.as_mut_ptr().cast(), len as u32)
        } else {
            HidD_GetInputReport(h.0, buf.as_mut_ptr().cast(), len as u32)
        }
    };
    if !ok {
        return None;
    }
    let mut value = 0u32;
    let st = unsafe { HidP_GetUsageValue(rtype, page, 0, usage, &mut value, pre, buf.as_ptr(), len as u32) };
    (st == HIDP_STATUS_SUCCESS).then_some(value)
}

fn hid_interfaces() -> Vec<String> {
    unsafe {
        let mut guid = std::mem::zeroed();
        HidD_GetHidGuid(&mut guid);
        let mut len = 0u32;
        if CM_Get_Device_Interface_List_SizeW(&mut len, &guid, std::ptr::null(), CM_GET_DEVICE_INTERFACE_LIST_PRESENT)
            != 0
            || len == 0
        {
            return vec![];
        }
        let mut buf = vec![0u16; len as usize];
        if CM_Get_Device_Interface_ListW(
            &guid,
            std::ptr::null(),
            buf.as_mut_ptr(),
            len,
            CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
        ) != 0
        {
            return vec![];
        }
        buf.split(|&c| c == 0).filter(|s| !s.is_empty()).map(String::from_utf16_lossy).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vid_pid() {
        let p = r"\\?\HID#VID_3151&PID_402D&MI_00#8&1c700cb7&0&0000#{378de44c-56ef-11d1-bc8c-00a0c91405dd}";
        assert_eq!(parse_vid_pid(p), (0x3151, 0x402D));
    }
}
