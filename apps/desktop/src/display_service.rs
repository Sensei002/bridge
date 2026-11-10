//! Bounded client for the local Bridge Display Service named pipe.
//!
//! The desktop process remains unprivileged. This module only relays a one-time
//! service challenge through an already-authenticated Android Noise session.

use std::{
    ffi::OsStr,
    io,
    os::windows::ffi::OsStrExt,
    ptr::null_mut,
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
    Storage::FileSystem::{
        CreateFileW, ReadFile, WriteFile, FILE_GENERIC_READ, FILE_GENERIC_WRITE, OPEN_EXISTING,
    },
    System::Pipes::WaitNamedPipeW,
};

const PIPE_NAME: &str = r"\\.\pipe\BridgeDisplayService";
const PIPE_WAIT_MS: u32 = 1_500;
const MAX_RESPONSE_BYTES: usize = 512;

struct OwnedPipe(HANDLE);

impl Drop for OwnedPipe {
    fn drop(&mut self) {
        if self.0 != INVALID_HANDLE_VALUE && !self.0.is_null() {
            unsafe { CloseHandle(self.0); }
        }
    }
}

/// Ask the service for a fresh process-pinned challenge.
pub fn issue_challenge() -> Result<String, String> {
    let response = request("CHALLENGE")?;
    let prefix = "BRIDGE_DISPLAY/1 CHALLENGE nonce=";
    let nonce = response.strip_prefix(prefix)
        .ok_or_else(|| service_response_error(&response))?
        .trim();
    if nonce.len() != 64 || !nonce.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("display service returned a malformed challenge".into());
    }
    Ok(nonce.to_ascii_lowercase())
}

/// Submit the Android-echoed challenge to the service on a new pipe connection.
pub fn submit_challenge_response(nonce: &str) -> Result<(), String> {
    if nonce.len() != 64 || !nonce.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("display challenge response must be 64 hexadecimal characters".into());
    }
    let response = request(&format!("CHALLENGE_RESPONSE {}", nonce))?;
    if response.trim() == "BRIDGE_DISPLAY/1 OK challenge_verified" {
        Ok(())
    } else {
        Err(service_response_error(&response))
    }
}

fn service_response_error(response: &str) -> String {
    response.trim()
        .strip_prefix("BRIDGE_DISPLAY/1 ERROR ")
        .map(str::to_owned)
        .unwrap_or_else(|| format!("unexpected display-service response: {}", response.trim()))
}

fn request(command: &str) -> Result<String, String> {
    if command.is_empty() || command.len() > 96
        || !command.is_ascii()
        || command.contains(['\r', '\n'])
    {
        return Err("invalid display-service command".into());
    }

    let name = OsStr::new(PIPE_NAME).encode_wide().chain(Some(0)).collect::<Vec<_>>();
    if unsafe { WaitNamedPipeW(name.as_ptr(), PIPE_WAIT_MS) } == 0 {
        return Err(format!("Bridge Display Service pipe is unavailable: {}", io::Error::last_os_error()));
    }

    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            FILE_GENERIC_READ | FILE_GENERIC_WRITE,
            0,
            null_mut(),
            OPEN_EXISTING,
            0,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(format!("could not open Bridge Display Service pipe: {}", io::Error::last_os_error()));
    }
    let pipe = OwnedPipe(handle);

    let request = format!("BRIDGE_DISPLAY/1 {command}");
    let mut written = 0_u32;
    let write_ok = unsafe {
        WriteFile(pipe.0, request.as_ptr(), request.len() as u32, &mut written, null_mut())
    };
    if write_ok == 0 || written as usize != request.len() {
        return Err(format!("could not write display-service request: {}", io::Error::last_os_error()));
    }

    let mut response = [0_u8; MAX_RESPONSE_BYTES];
    let mut read = 0_u32;
    let read_ok = unsafe {
        ReadFile(pipe.0, response.as_mut_ptr(), response.len() as u32, &mut read, null_mut())
    };
    if read_ok == 0 {
        return Err(format!("could not read display-service response: {}", io::Error::last_os_error()));
    }
    if read == 0 || read as usize > MAX_RESPONSE_BYTES {
        return Err("display service returned an empty or oversized response".into());
    }
    String::from_utf8(response[..read as usize].to_vec())
        .map_err(|_| "display service returned a non-UTF-8 response".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_well_formed_challenge_responses() {
        let nonce = "aB".repeat(32);
        let response = format!("BRIDGE_DISPLAY/1 CHALLENGE nonce={nonce}");
        assert_eq!(
            response.strip_prefix("BRIDGE_DISPLAY/1 CHALLENGE nonce=").unwrap().trim().to_ascii_lowercase(),
            nonce.to_ascii_lowercase()
        );
        assert!(service_response_error("BRIDGE_DISPLAY/1 ERROR signer pin missing").contains("signer pin missing"));
        assert!(service_response_error("unexpected").contains("unexpected"));
    }
}
