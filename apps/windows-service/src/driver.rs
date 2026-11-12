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

const IOCTL_READ_FRAME: u32 = (0x22 << 16) | (0x803 << 2) | (2 << 14) | (1 << 14);
const IOCTL_QUERY_FRAME: u32 = (0x22 << 16) | (0x804 << 2) | (1 << 14);
const FRAME_HEADER_SIZE: usize = 48;
const FRAME_MAX_BYTES: usize = 64 * 1024 * 1024;
const FRAME_PIXEL_FORMAT_BGRA8: u32 = 1;
const CM_GET_DEVICE_INTERFACE_LIST_PRESENT: u32 = 0;
const CR_SUCCESS: u32 = 0;

const BRIDGE_DISPLAY_INTERFACE_GUID: GUID = GUID {
    data1: 0x7f0d8b31,
    data2: 0x5c4a,
    data3: 0x4a72,
    data4: [0x9d, 0x8f, 0x43, 0x21, 0x6b, 0x70, 0x2e, 0xa1],
};

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




/// Metadata for one complete BGRA8 frame captured by the IddCx swap-chain worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub pixel_format: u32,
    pub sequence: u64,
    pub timestamp_ns: u64,
    pub data_size: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedFrame {
    pub header: FrameHeader,
    pub pixels: Vec<u8>,
}

impl CapturedFrame {
    /// Serialize the fixed driver header followed by its BGRA8 pixel payload.
    pub fn into_packet(self) -> Vec<u8> {
        let mut packet = Vec::with_capacity(FRAME_HEADER_SIZE + self.pixels.len());
        for value in [FRAME_HEADER_SIZE as u32, 1, self.header.width, self.header.height, self.header.stride, self.header.pixel_format] {
            packet.extend_from_slice(&value.to_le_bytes());
        }
        packet.extend_from_slice(&self.header.sequence.to_le_bytes());
        packet.extend_from_slice(&self.header.timestamp_ns.to_le_bytes());
        packet.extend_from_slice(&self.header.data_size.to_le_bytes());
        packet.extend_from_slice(&0_u32.to_le_bytes());
        packet.extend_from_slice(&self.pixels);
        packet
    }
}

fn read_u32(bytes: &[u8], offset: usize) -> io::Result<u32> {
    let value = bytes.get(offset..offset + 4)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "truncated frame header"))?;
    Ok(u32::from_le_bytes(value.try_into().map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidData, "invalid frame header field")
    })?))
}

fn read_u64(bytes: &[u8], offset: usize) -> io::Result<u64> {
    let value = bytes.get(offset..offset + 8)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "truncated frame header"))?;
    Ok(u64::from_le_bytes(value.try_into().map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidData, "invalid frame header field")
    })?))
}

/// Decode and strictly validate the fixed 48-byte driver frame header.
fn decode_frame_header(bytes: &[u8]) -> io::Result<FrameHeader> {
    if bytes.len() < FRAME_HEADER_SIZE {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "truncated frame header"));
    }
    let size = read_u32(bytes, 0)?;
    let version = read_u32(bytes, 4)?;
    let width = read_u32(bytes, 8)?;
    let height = read_u32(bytes, 12)?;
    let stride = read_u32(bytes, 16)?;
    let pixel_format = read_u32(bytes, 20)?;
    let sequence = read_u64(bytes, 24)?;
    let timestamp_ns = read_u64(bytes, 32)?;
    let data_size = read_u32(bytes, 40)?;
    let reserved = read_u32(bytes, 44)?;

    if size as usize != FRAME_HEADER_SIZE || version != 1 || reserved != 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "unsupported frame header version or size"));
    }
    if width == 0 || height == 0 || width > 4096 || height > 4096 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame dimensions are outside supported bounds"));
    }
    let expected_stride = width.checked_mul(4)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "frame stride overflow"))?;
    if stride != expected_stride || pixel_format != FRAME_PIXEL_FORMAT_BGRA8 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "unsupported frame stride or pixel format"));
    }
    let expected_size = (stride as u64) * (height as u64);
    if expected_size > FRAME_MAX_BYTES as u64 || data_size as u64 != expected_size || sequence == 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid frame payload size or sequence"));
    }

    Ok(FrameHeader { width, height, stride, pixel_format, sequence, timestamp_ns, data_size })
}

/// Read the newest complete frame. A frame may advance between metadata and
/// payload IOCTLs, so retry a bounded number of times if the payload grew.
pub fn read_latest_frame() -> io::Result<CapturedFrame> {
    with_driver(|handle| {
        for _attempt in 0..3 {
            let mut header_bytes = [0_u8; FRAME_HEADER_SIZE];
            let mut returned = 0_u32;
            let queried = unsafe {
                DeviceIoControl(
                    handle,
                    IOCTL_QUERY_FRAME,
                    null(),
                    0,
                    header_bytes.as_mut_ptr() as *mut _,
                    FRAME_HEADER_SIZE as u32,
                    &mut returned,
                    null_mut(),
                )
            };
            if queried == 0 {
                return Err(io::Error::last_os_error());
            }
            if returned as usize != FRAME_HEADER_SIZE {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "driver returned an incomplete frame header"));
            }
            let header = decode_frame_header(&header_bytes)?;
            let total_size = FRAME_HEADER_SIZE + header.data_size as usize;
            let mut packet = vec![0_u8; total_size];
            let read_ok = unsafe {
                DeviceIoControl(
                    handle,
                    IOCTL_READ_FRAME,
                    null(),
                    0,
                    packet.as_mut_ptr() as *mut _,
                    packet.len() as u32,
                    &mut returned,
                    null_mut(),
                )
            };
            if read_ok == 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(122) {
                    continue;
                }
                return Err(error);
            }
            if (returned as usize) < FRAME_HEADER_SIZE || (returned as usize) > packet.len() {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "driver returned an invalid frame packet length"));
            }
            packet.truncate(returned as usize);
            let actual_header = decode_frame_header(&packet)?;
            let payload = &packet[FRAME_HEADER_SIZE..];
            if payload.len() != actual_header.data_size as usize {
                continue;
            }
            return Ok(CapturedFrame { header: actual_header, pixels: payload.to_vec() });
        }
        Err(io::Error::new(io::ErrorKind::WouldBlock, "frame changed during readback; retry on the next poll"))
    })
}

#[cfg(test)]
mod frame_tests {
    use super::*;

    fn valid_header() -> Vec<u8> {
        let mut bytes = Vec::with_capacity(FRAME_HEADER_SIZE);
        bytes.extend_from_slice(&(FRAME_HEADER_SIZE as u32).to_le_bytes());
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&2_u32.to_le_bytes());
        bytes.extend_from_slice(&2_u32.to_le_bytes());
        bytes.extend_from_slice(&8_u32.to_le_bytes());
        bytes.extend_from_slice(&FRAME_PIXEL_FORMAT_BGRA8.to_le_bytes());
        bytes.extend_from_slice(&7_u64.to_le_bytes());
        bytes.extend_from_slice(&123_u64.to_le_bytes());
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes
    }

    #[test]
    fn validates_and_decodes_bgra_frame_header() {
        let header = decode_frame_header(&valid_header()).unwrap();
        assert_eq!(header.width, 2);
        assert_eq!(header.height, 2);
        assert_eq!(header.stride, 8);
        assert_eq!(header.sequence, 7);
        assert_eq!(header.data_size, 16);
    }

    #[test]
    fn rejects_truncated_headers_and_unknown_versions() {
        assert!(decode_frame_header(&[0; 12]).is_err());
        let mut bytes = valid_header();
        bytes[4..8].copy_from_slice(&2_u32.to_le_bytes());
        assert!(decode_frame_header(&bytes).is_err());
    }

    #[test]
    fn rejects_oversized_dimensions_and_mismatched_payload_lengths() {
        let mut bytes = valid_header();
        bytes[8..12].copy_from_slice(&8192_u32.to_le_bytes());
        assert!(decode_frame_header(&bytes).is_err());
        let mut bytes = valid_header();
        bytes[40..44].copy_from_slice(&15_u32.to_le_bytes());
        assert!(decode_frame_header(&bytes).is_err());
    }

    #[test]
    fn rejects_unknown_pixel_formats_and_zero_sequences() {
        let mut bytes = valid_header();
        bytes[20..24].copy_from_slice(&99_u32.to_le_bytes());
        assert!(decode_frame_header(&bytes).is_err());
        let mut bytes = valid_header();
        bytes[24..32].copy_from_slice(&0_u64.to_le_bytes());
        assert!(decode_frame_header(&bytes).is_err());
    }
}
