use std::{io, mem::size_of, ptr::{null, null_mut}};
use windows_sys::{
    core::GUID,
    Win32::{
        Devices::DeviceAndDriverInstallation::{
            CM_Get_Device_Interface_ListW, CM_Get_Device_Interface_List_SizeW,
        },
        Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
        Storage::FileSystem::{
            CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        },
        System::IO::DeviceIoControl,
    },
};

const IOCTL_CONNECT: u32 = (0x22 << 16) | (0x800 << 2) | (2 << 14);
const IOCTL_DISCONNECT: u32 = (0x22 << 16) | (0x801 << 2) | (2 << 14);
const IOCTL_QUERY: u32 = (0x22 << 16) | (0x802 << 2) | (1 << 14);
const CM_GET_DEVICE_INTERFACE_LIST_PRESENT: u32 = 0;
const CR_SUCCESS: u32 = 0;

const BRIDGE_DISPLAY_INTERFACE_GUID: GUID = GUID {
    data1: 0x7f0d8b31,
    data2: 0x5c4a,
    data3: 0x4a72,
    data4: [0x9d, 0x8f, 0x43, 0x21, 0x6b, 0x70, 0x2e, 0xa1],
};

#[repr(C)]
#[derive(Default, Debug, Clone, Copy)]
pub struct DriverStatus {
    pub size: u32,
    pub version: u32,
    pub is_connected: u32,
    pub supported_mode_count: u32,
}

fn interface_path() -> io::Result<Vec<u16>> {
    let mut chars = 0_u32;
    let result = unsafe {
        CM_Get_Device_Interface_List_SizeW(
            &mut chars,
            &BRIDGE_DISPLAY_INTERFACE_GUID,
            null(),
            CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
        )
    };
    if result != CR_SUCCESS || chars < 2 {
        return Err(io::Error::new(io::ErrorKind::NotFound, "Bridge display driver interface is not present"));
    }

    let mut paths = vec![0_u16; chars as usize];
    let result = unsafe {
        CM_Get_Device_Interface_ListW(
            &BRIDGE_DISPLAY_INTERFACE_GUID,
            null(),
            paths.as_mut_ptr(),
            chars,
            CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
        )
    };
    if result != CR_SUCCESS {
        return Err(io::Error::new(io::ErrorKind::NotFound, format!("could not enumerate Bridge driver interface (CONFIGRET {result})")));
    }

    let end = paths.iter().position(|value| *value == 0)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "driver returned an invalid interface path"))?;
    if end == 0 {
        return Err(io::Error::new(io::ErrorKind::NotFound, "Bridge display driver interface is not present"));
    }
    paths.truncate(end + 1);
    Ok(paths)
}

fn with_driver<T>(operation: impl FnOnce(HANDLE) -> io::Result<T>) -> io::Result<T> {
    let path = interface_path()?;
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            0x8000_0000 | 0x4000_0000,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let result = operation(handle);
    unsafe { CloseHandle(handle); }
    result
}

pub fn query() -> io::Result<DriverStatus> {
    with_driver(|handle| {
        let mut status = DriverStatus { size: size_of::<DriverStatus>() as u32, ..Default::default() };
        let mut returned = 0_u32;
        let ok = unsafe {
            DeviceIoControl(
                handle,
                IOCTL_QUERY,
                null(),
                0,
                &mut status as *mut DriverStatus as *mut _,
                size_of::<DriverStatus>() as u32,
                &mut returned,
                null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        if returned as usize != size_of::<DriverStatus>() || status.version != 1 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "driver control protocol version or response size mismatch"));
        }
        Ok(status)
    })
}

pub fn set_connected(connected: bool) -> io::Result<()> {
    with_driver(|handle| {
        let mut returned = 0_u32;
        let code = if connected { IOCTL_CONNECT } else { IOCTL_DISCONNECT };
        let ok = unsafe {
            DeviceIoControl(handle, code, null(), 0, null_mut(), 0, &mut returned, null_mut())
        };
        if ok == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    })
}
