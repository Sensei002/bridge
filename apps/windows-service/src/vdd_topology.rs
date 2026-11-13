//! Read-only Windows PnP probe for the upstream VirtualDrivers VDD.
//!
//! Windows exposes the device instance as ROOT\DISPLAY\000x, while the driver's
//! hardware ID (Root\MttVDD) is stored separately in SPDRP_HARDWAREID. Matching
//! DISPLAY_DEVICEW.DeviceID against the hardware ID therefore misses the device
//! and can cause Bridge to install duplicate root-enumerated instances.

use std::mem::{size_of, zeroed};
use windows_sys::Win32::{
    Devices::DeviceAndDriverInstallation::{
        SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
        SetupDiGetDeviceRegistryPropertyW, SP_DEVINFO_DATA, DIGCF_ALLCLASSES,
        DIGCF_PRESENT,
    },
    Foundation::GetLastError,
    Graphics::Gdi::{EnumDisplayDevicesW, DISPLAY_DEVICEW, DISPLAY_DEVICE_ATTACHED_TO_DESKTOP},
};

const MAX_PNP_DEVICES: u32 = 4096;
const SPDRP_HARDWAREID: u32 = 0x00000001;
const ERROR_INSUFFICIENT_BUFFER: u32 = 122;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VddTopology {
    pub present: bool,
    pub attached_to_desktop: bool,
}

fn is_vdd_hardware_id(value: &str) -> bool {
    value.trim_matches(char::from(0)).eq_ignore_ascii_case(r"ROOT\MttVDD")
}

fn has_vdd_hardware_id(buffer: &[u16]) -> bool {
    String::from_utf16_lossy(buffer).split('\0').any(is_vdd_hardware_id)
}

/// Query present PnP devices and inspect their actual hardware-ID property.
/// The display instance ID (ROOT\DISPLAY\xxxx) is deliberately not used.
fn vdd_pnp_device_present() -> bool {
    let devices = unsafe {
        SetupDiGetClassDevsW(
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            DIGCF_PRESENT | DIGCF_ALLCLASSES,
        )
    };
    if devices == -1isize {
        return false;
    }

    let mut present = false;
    for index in 0..MAX_PNP_DEVICES {
        let mut device: SP_DEVINFO_DATA = unsafe { zeroed() };
        device.cbSize = size_of::<SP_DEVINFO_DATA>() as u32;
        if unsafe { SetupDiEnumDeviceInfo(devices, index, &mut device) } == 0 {
            break;
        }

        let mut required = 0_u32;
        unsafe {
            SetupDiGetDeviceRegistryPropertyW(
                devices, &device, SPDRP_HARDWAREID, std::ptr::null_mut(),
                std::ptr::null_mut(), 0, &mut required,
            );
        }
        if required == 0 || unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER {
            continue;
        }

        let mut bytes = vec![0_u8; required as usize];
        let read = unsafe {
            SetupDiGetDeviceRegistryPropertyW(
                devices, &device, SPDRP_HARDWAREID, std::ptr::null_mut(),
                bytes.as_mut_ptr(), required, &mut required,
            )
        };
        if read != 0 {
            let words = unsafe {
                std::slice::from_raw_parts(bytes.as_ptr().cast::<u16>(), bytes.len() / 2)
            };
            if has_vdd_hardware_id(words) {
                present = true;
                break;
            }
        }
    }

    unsafe { SetupDiDestroyDeviceInfoList(devices); }
    present
}

/// Check whether a root-enumerated display adapter is attached to the desktop.
/// Presence is determined independently from the PnP hardware ID above.
fn any_root_display_attached() -> bool {
    for index in 0..64 {
        let mut device: DISPLAY_DEVICEW = unsafe { zeroed() };
        device.cb = size_of::<DISPLAY_DEVICEW>() as u32;
        if unsafe { EnumDisplayDevicesW(std::ptr::null(), index, &mut device, 0) } == 0 {
            break;
        }
        let end = device.DeviceID.iter().position(|unit| *unit == 0).unwrap_or(device.DeviceID.len());
        let instance_id = String::from_utf16_lossy(&device.DeviceID[..end]);
        if instance_id.to_ascii_uppercase().starts_with(r"ROOT\DISPLAY\")
            && (device.StateFlags & DISPLAY_DEVICE_ATTACHED_TO_DESKTOP) != 0
        {
            return true;
        }
    }
    false
}

/// Detect an existing MttVDD device by its hardware ID so it is reused instead
/// of reinstalling another root-enumerated instance on every activation.
pub fn query() -> VddTopology {
    let present = vdd_pnp_device_present();
    VddTopology {
        present,
        attached_to_desktop: present && any_root_display_attached(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_hardware_id_not_display_instance_id() {
        assert!(is_vdd_hardware_id(r"ROOT\MttVDD"));
        assert!(is_vdd_hardware_id(r"root\mttvdd"));
        assert!(!is_vdd_hardware_id(r"ROOT\DISPLAY\0000"));
        assert!(!is_vdd_hardware_id(r"PCI\VEN_1234&DEV_5678"));
    }

    #[test]
    fn finds_hardware_id_inside_multi_string_property() {
        let value = format!("{}\0{}\0\0", r"ROOT\MttVDD", r"ROOT\OTHER");
        let words = value.encode_utf16().collect::<Vec<_>>();
        assert!(has_vdd_hardware_id(&words));
        let other = r"ROOT\DISPLAY\0000".encode_utf16().collect::<Vec<_>>();
        assert!(!has_vdd_hardware_id(&other));
    }
}