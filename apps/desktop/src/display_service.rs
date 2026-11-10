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
    Storage::FileSystem::{CreateFileW, ReadFile, WriteFile, FILE_GENERIC_READ, FILE_GENERIC_WRITE, OPEN_EXISTING},
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

/// A short-lived lease minted by the Windows service after Android proof verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceLease {
    pub id: uuid::Uuid,
    pub expires_in_ms: u64,
}

/// Ask the service for a fresh process-pinned challenge.
pub fn issue_challenge() -> Result<String, String> {
    parse_challenge(&request("CHALLENGE")?)
}

/// Ask the service to pin the live Noise session and pairing-trusted Android key.
pub fn issue_attestation_challenge(
    session_id: uuid::Uuid,
    device_fingerprint: &str,
    pinned_public_key: &str,
) -> Result<String, String> {
    if !is_hex(device_fingerprint, 64) || !is_hex(pinned_public_key, 64) {
        return Err("invalid pairing-pinned Android identity for service challenge".into());
    }
    let command = format!(
        "CHALLENGE_ATTESTATION session={session_id} fingerprint={} public_key={}",
        device_fingerprint.to_ascii_lowercase(),
        pinned_public_key.to_ascii_lowercase(),
    );
    parse_challenge(&request(&command)?)
}

/// Submit the signed proof; the service verifies it independently and returns
/// the process- and peer-bound lease minted after successful verification.
pub fn submit_attestation_response(
    nonce: &str,
    session_id: uuid::Uuid,
    device_fingerprint: &str,
    signature: &str,
) -> Result<ServiceLease, String> {
    if !is_hex(nonce, 64) || !is_hex(device_fingerprint, 64) || !is_hex(signature, 128) {
        return Err("invalid Android attestation response encoding".into());
    }
    let command = format!(
        "ATTESTATION_RESPONSE nonce={} session={session_id} fingerprint={} signature={}",
        nonce.to_ascii_lowercase(),
        device_fingerprint.to_ascii_lowercase(),
        signature.to_ascii_lowercase(),
    );
    let response = request(&command)?;
    parse_lease_response(&response)
}

fn parse_lease_response(response: &str) -> Result<ServiceLease, String> {
    let value = response.trim()
        .strip_prefix("BRIDGE_DISPLAY/1 OK lease_issued id=")
        .or_else(|| response.trim().strip_prefix("BRIDGE_DISPLAY/1 OK lease_renewed id="))
        .ok_or_else(|| service_response_error(response))?;
    let (lease_id, ttl) = value.split_once(" expires_in_ms=")
        .ok_or_else(|| "display service returned a malformed lease".to_string())?;
    let id = uuid::Uuid::parse_str(lease_id)
        .map_err(|_| "display service returned an invalid lease ID".to_string())?;
    let expires_in_ms: u64 = ttl.parse()
        .map_err(|_| "display service returned an invalid lease lifetime".to_string())?;
    if expires_in_ms == 0 || expires_in_ms > 15_000 {
        return Err("display service returned an out-of-policy lease lifetime".into());
    }
    Ok(ServiceLease { id, expires_in_ms })
}

/// Renew the same lease while its original process and Android peer remain authenticated.
pub fn renew_with_lease(lease: &ServiceLease, device_fingerprint: &str) -> Result<ServiceLease, String> {
    if lease.expires_in_ms == 0 || lease.expires_in_ms > 15_000 || !is_hex(device_fingerprint, 64) {
        return Err("invalid or expired display lease identity".into());
    }
    let command = format!(
        "RENEW lease={} fingerprint={}",
        lease.id,
        device_fingerprint.to_ascii_lowercase(),
    );
    let response = request(&command)?;
    parse_lease_response(&response)
}

/// Request a driver connection using the exact service-issued lease identity.
pub fn connect_with_lease(lease: &ServiceLease, device_fingerprint: &str) -> Result<(), String> {
    mutate_with_lease("CONNECT", "connected", lease, device_fingerprint)
}

/// Request a driver disconnection using the exact service-issued lease identity.
pub fn disconnect_with_lease(lease: &ServiceLease, device_fingerprint: &str) -> Result<(), String> {
    mutate_with_lease("DISCONNECT", "disconnected", lease, device_fingerprint)
}

fn mutate_with_lease(
    operation: &str,
    expected_status: &str,
    lease: &ServiceLease,
    device_fingerprint: &str,
) -> Result<(), String> {
    if lease.expires_in_ms == 0 || lease.expires_in_ms > 15_000 || !is_hex(device_fingerprint, 64) {
        return Err("invalid or expired display lease identity".into());
    }
    let command = format!(
        "{operation} lease={} fingerprint={}",
        lease.id,
        device_fingerprint.to_ascii_lowercase(),
    );
    let response = request(&command)?;
    let expected = format!("BRIDGE_DISPLAY/1 OK {expected_status}");
    if response.trim() == expected {
        Ok(())
    } else {
        Err(service_response_error(&response))
    }
}

fn is_hex(value: &str, expected_len: usize) -> bool {
    value.len() == expected_len && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn parse_challenge(response: &str) -> Result<String, String> {
    let prefix = "BRIDGE_DISPLAY/1 CHALLENGE nonce=";
    let nonce = response.strip_prefix(prefix)
        .ok_or_else(|| service_response_error(response))?
        .trim();
    if nonce.len() != 64 || !nonce.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("display service returned a malformed challenge".into());
    }
    Ok(nonce.to_ascii_lowercase())
}

fn service_response_error(response: &str) -> String {
    response.trim()
        .strip_prefix("BRIDGE_DISPLAY/1 ERROR ")
        .map(str::to_owned)
        .unwrap_or_else(|| format!("unexpected display-service response: {}", response.trim()))
}

fn request(command: &str) -> Result<String, String> {
    if command.is_empty() || command.len() > 384
        || !command.is_ascii()
        || command.contains('\r') || command.contains('\n')
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
        assert_eq!(parse_challenge(&response).unwrap(), nonce.to_ascii_lowercase());
        assert!(parse_challenge("BRIDGE_DISPLAY/1 CHALLENGE nonce=invalid").is_err());
        assert!(parse_challenge("BRIDGE_DISPLAY/1 ERROR signer pin missing").unwrap_err().contains("signer pin missing"));
        assert!(service_response_error("BRIDGE_DISPLAY/1 ERROR signer pin missing").contains("signer pin missing"));
        assert!(service_response_error("unexpected").contains("unexpected"));
    }

    #[test]
    fn accepts_only_valid_short_lived_service_leases() {
        let id = uuid::Uuid::new_v4();
        let response = format!("BRIDGE_DISPLAY/1 OK lease_issued id={id} expires_in_ms=15000");
        assert_eq!(parse_lease_response(&response).unwrap(), ServiceLease { id, expires_in_ms: 15_000 });
        let renewed = format!("BRIDGE_DISPLAY/1 OK lease_renewed id={id} expires_in_ms=14999");
        assert_eq!(parse_lease_response(&renewed).unwrap(), ServiceLease { id, expires_in_ms: 14_999 });
        assert!(parse_lease_response(&format!(
            "BRIDGE_DISPLAY/1 OK lease_issued id={id} expires_in_ms=0"
        )).is_err());
        assert!(parse_lease_response(&format!(
            "BRIDGE_DISPLAY/1 OK lease_issued id={id} expires_in_ms=15001"
        )).is_err());
        assert!(parse_lease_response(
            "BRIDGE_DISPLAY/1 OK lease_issued id=not-a-uuid expires_in_ms=15000"
        ).is_err());
        assert!(parse_lease_response(
            "BRIDGE_DISPLAY/1 OK attestation_verified"
        ).is_err());
    }
}
