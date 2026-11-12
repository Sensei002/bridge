//! Read-only Windows display-topology probe for the upstream VirtualDrivers VDD.
//!
//! The vendor pipe only accepts display-count requests and provides no
//! acknowledgement. This module independently checks the PnP display adapter
//! list so Bridge never reports a successful monitor connection from pipe-write
//! success alone.

use std::{io, mem::{size_of, zeroed}};
use windows_sys::Win32::Graphics::Gdi::{
    EnumDisplayDevicesW, DISPLAY_DEVICEW, DISPLAY_DEVICE_ATTACHED_TO_DESKTOP,
};

const MAX_DISPLAY_ADAPTERS: u32 = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VddTopology {
    pub present: bool,
    pub attached_to_desktop: bool,
}

fn field_string(value: &[u16]) -> String {
    let end = value.iter().position(|unit| *unit == 0).unwrap_or(value.len());
    String::from_utf16_lossy(&value[..end])
}

fn is_vdd_device_id(value: &str) -> bool {
    value.to_ascii_uppercase().starts_with("ROOT\\MTTVDD\\")
        || value.eq_ignore_ascii_case("ROOT\\MTTVDD")
}

/// Enumerate display adapters and match the exact hardware ID from the pinned
/// upstream INF (Root\MttVDD), never a display index or friendly-name guess.
pub fn query() -> io::Result<VddTopology> {
    let mut topology = VddTopology { present: false, attached_to_desktop: false };

    for index in 0..MAX_DISPLAY_ADAPTERS {
        let mut device: DISPLAY_DEVICEW = unsafe { zeroed() };
        device.cb = size_of::<DISPLAY_DEVICEW>() as u32;
        let found = unsafe { EnumDisplayDevicesW(std::ptr::null(), index, &mut device, 0) };
        if found == 0 {
            break;
        }

        if is_vdd_device_id(&field_string(&device.DeviceID)) {
            topology.present = true;
            topology.attached_to_desktop = (device.StateFlags & DISPLAY_DEVICE_ATTACHED_TO_DESKTOP) != 0;
            break;
        }
    }

    Ok(topology)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_only_the_upstream_vdd_hardware_id() {
        assert!(is_vdd_device_id("ROOT\\MttVDD\\0000"));
        assert!(is_vdd_device_id("root\\mttvdd"));
        assert!(!is_vdd_device_id("ROOT\\DISPLAY\\0000"));
        assert!(!is_vdd_device_id("PCI\\VEN_1234&DEV_5678"));
    }

    #[test]
    fn safely_decodes_null_terminated_display_fields() {
        assert_eq!(field_string(&[b'V' as u16, b'D' as u16, 0, b'X' as u16]), "VD");
    }
}
