//! Narrow client for the upstream VirtualDrivers VDD control pipe.
//!
//! The upstream driver exposes a message-mode UTF-16 named pipe. Only the
//! display-count command is supported here; arbitrary upstream control commands
//! are deliberately not forwarded from the Bridge UI or Android peer.

use std::{io, thread, time::Duration};

use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY, HANDLE, INVALID_HANDLE_VALUE},
    Storage::FileSystem::{CreateFileW, WriteFile, FILE_ATTRIBUTE_NORMAL, OPEN_EXISTING},
    System::Pipes::WaitNamedPipeW,
};

const PIPE_NAME: &str = r"\\.\pipe\MTTVirtualDisplayPipe";
const PIPE_RETRY_COUNT: usize = 10;
const PIPE_RETRY_DELAY: Duration = Duration::from_millis(150);
const PIPE_WAIT_MS: u32 = 500;

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn display_count_command(count: u32) -> io::Result<Vec<u8>> {
    if count > 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Bridge supports exactly zero or one upstream virtual display",
        ));
    }

    let command = wide(&format!("SETDISPLAYCOUNT {count}"));
    let bytes = unsafe {
        std::slice::from_raw_parts(command.as_ptr().cast::<u8>(), command.len() * 2)
    };
    Ok(bytes.to_vec())
}

/// Set the upstream VDD display count to zero (disconnect) or one (connect).
///
/// The upstream protocol accepts one UTF-16 message per pipe connection and
/// does not acknowledge SETDISPLAYCOUNT. Success therefore means the command
/// was delivered to the driver, not that Windows has finished enumerating the
/// monitor. Callers must verify the resulting display topology separately.
pub fn set_display_count(count: u32) -> io::Result<()> {
    let payload = display_count_command(count)?;
    let pipe_name = wide(PIPE_NAME);
    let mut last_error = None;

    for attempt in 0..PIPE_RETRY_COUNT {
        let handle: HANDLE = unsafe {
            CreateFileW(
                pipe_name.as_ptr(),
                0x4000_0000 | 0x8000_0000,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                std::ptr::null_mut(),
            )
        };

        if handle != INVALID_HANDLE_VALUE {
            let mut written = 0_u32;
            let ok = unsafe {
                WriteFile(
                    handle,
                    payload.as_ptr(),
                    payload.len() as u32,
                    &mut written,
                    std::ptr::null_mut(),
                )
            };
            let write_error = if ok == 0 {
                Some(io::Error::last_os_error())
            } else if written as usize != payload.len() {
                Some(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "upstream VDD pipe accepted only part of the command",
                ))
            } else {
                None
            };
            unsafe { CloseHandle(handle); }
            return write_error.map_or(Ok(()), Err);
        }

        let error = io::Error::last_os_error();
        let code = error.raw_os_error();
        last_error = Some(error);

        if code != Some(ERROR_PIPE_BUSY as i32) && code != Some(ERROR_FILE_NOT_FOUND as i32) {
            break;
        }

        if code == Some(ERROR_PIPE_BUSY as i32) {
            unsafe { WaitNamedPipeW(pipe_name.as_ptr(), PIPE_WAIT_MS); }
        } else if attempt + 1 < PIPE_RETRY_COUNT {
            thread::sleep(PIPE_RETRY_DELAY);
        }
    }

    Err(last_error.unwrap_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "upstream VDD control pipe is unavailable")
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_the_upstream_utf16_display_count_message() {
        let bytes = display_count_command(1).unwrap();
        let words = bytes.chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        assert_eq!(String::from_utf16(&words[..words.len() - 1]).unwrap(), "SETDISPLAYCOUNT 1");
        assert_eq!(words.last(), Some(&0));
    }

    #[test]
    fn supports_disconnect_and_rejects_unbounded_display_counts() {
        assert!(display_count_command(0).is_ok());
        assert_eq!(display_count_command(2).unwrap_err().kind(), io::ErrorKind::InvalidInput);
    }
}
