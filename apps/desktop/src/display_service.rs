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
const FRAME_HEADER_BYTES: usize = 48;
const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;
const MAX_FRAME_PACKET_BYTES: usize = FRAME_HEADER_BYTES + MAX_FRAME_BYTES;

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

/// A validated BGRA8 frame copied from the privileged Windows display service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceFrame {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub sequence: u64,
    pub timestamp_ns: u64,
    pub pixels: Vec<u8>,
}

/// Read one complete frame through the service's lease-authorized named pipe.
pub fn read_frame_with_lease(lease: &ServiceLease, device_fingerprint: &str) -> Result<ServiceFrame, String> {
    if lease.expires_in_ms == 0 || lease.expires_in_ms > 15_000 || !is_hex(device_fingerprint, 64) {
        return Err("invalid or expired display lease identity".into());
    }
    let command = format!("READ_FRAME lease={} fingerprint={}", lease.id, device_fingerprint.to_ascii_lowercase());
    let packet = request_bytes(&command, MAX_FRAME_PACKET_BYTES)?;
    decode_frame_packet(&packet)
}

fn decode_frame_packet(packet: &[u8]) -> Result<ServiceFrame, String> {
    if packet.len() < FRAME_HEADER_BYTES {
        return Err("display service returned a truncated frame header".into());
    }
    let read_u32 = |offset: usize| -> Result<u32, String> {
        let bytes: [u8; 4] = packet.get(offset..offset + 4).ok_or_else(|| "truncated frame header".to_string())?
            .try_into().map_err(|_| "invalid frame header field".to_string())?;
        Ok(u32::from_le_bytes(bytes))
    };
    let read_u64 = |offset: usize| -> Result<u64, String> {
        let bytes: [u8; 8] = packet.get(offset..offset + 8).ok_or_else(|| "truncated frame header".to_string())?
            .try_into().map_err(|_| "invalid frame header field".to_string())?;
        Ok(u64::from_le_bytes(bytes))
    };
    let size = read_u32(0)? as usize;
    let version = read_u32(4)?;
    let width = read_u32(8)?;
    let height = read_u32(12)?;
    let stride = read_u32(16)?;
    let pixel_format = read_u32(20)?;
    let sequence = read_u64(24)?;
    let timestamp_ns = read_u64(32)?;
    let data_size = read_u32(40)? as usize;
    let reserved = read_u32(44)?;
    let expected_stride = width.checked_mul(4).ok_or_else(|| "frame stride overflow".to_string())?;
    let expected_size = (stride as u64) * (height as u64);
    if size != FRAME_HEADER_BYTES || version != 1 || reserved != 0
        || width == 0 || height == 0 || width > 4096 || height > 4096
        || stride != expected_stride || pixel_format != 1 || sequence == 0
        || expected_size > MAX_FRAME_BYTES as u64 || data_size != expected_size as usize
        || packet.len() != FRAME_HEADER_BYTES + data_size
    {
        return Err("display service returned invalid frame metadata or payload length".into());
    }
    Ok(ServiceFrame {
        width, height, stride, sequence, timestamp_ns,
        pixels: packet[FRAME_HEADER_BYTES..].to_vec(),
    })
}


/// Actual upstream VDD presence and Windows desktop-attachment state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VddTopologyStatus {
    pub present: bool,
    pub attached_to_desktop: bool,
}

/// Query Windows display topology without changing monitor or lease state.
pub fn query_status() -> Result<VddTopologyStatus, String> {
    parse_vdd_topology(&request("QUERY")?)
}

fn parse_vdd_topology(response: &str) -> Result<VddTopologyStatus, String> {
    let value = response.trim()
        .strip_prefix("BRIDGE_DISPLAY/1 OK vdd_present=")
        .ok_or_else(|| service_response_error(response))?;
    let (present, attached) = value.split_once(" attached=")
        .ok_or_else(|| "display service returned malformed VDD topology".to_string())?;
    let parse_flag = |value: &str| match value {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err("display service returned an invalid topology flag".to_string()),
    };
    let present = parse_flag(present)?;
    let attached_to_desktop = parse_flag(attached)?;
    if attached_to_desktop && !present {
        return Err("display service reported an attached VDD that is not present".into());
    }
    Ok(VddTopologyStatus { present, attached_to_desktop })
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
    let renewed = parse_lease_response(&response)?;
    if renewed.id != lease.id {
        return Err("display service changed the active lease ID during renewal".into());
    }
    Ok(renewed)
}

/// Request a driver connection using the exact service-issued lease identity.
pub fn connect_with_lease(lease: &ServiceLease, device_fingerprint: &str) -> Result<(), String> {
    mutate_with_lease("CONNECT", "connect_requested", lease, device_fingerprint)
}

/// Request a driver disconnection using the exact service-issued lease identity.
pub fn disconnect_with_lease(lease: &ServiceLease, device_fingerprint: &str) -> Result<(), String> {
    mutate_with_lease("DISCONNECT", "disconnect_requested", lease, device_fingerprint)
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
    let bytes = request_bytes(command, MAX_RESPONSE_BYTES)?;
    String::from_utf8(bytes).map_err(|_| "display service returned a non-UTF-8 response".into())
}

fn request_bytes(command: &str, response_capacity: usize) -> Result<Vec<u8>, String> {
    if command.is_empty() || command.len() > 384 || !command.is_ascii()
        || command.contains('\r') || command.contains('\n')
    {
        return Err("invalid display-service command".into());
    }
    if response_capacity == 0 || response_capacity > MAX_FRAME_PACKET_BYTES {
        return Err("invalid display-service response limit".into());
    }
    let name = OsStr::new(PIPE_NAME).encode_wide().chain(Some(0)).collect::<Vec<_>>();
    if unsafe { WaitNamedPipeW(name.as_ptr(), PIPE_WAIT_MS) } == 0 {
        return Err(format!("Bridge Display Service pipe is unavailable: {}", io::Error::last_os_error()));
    }
    let handle = unsafe {
        CreateFileW(name.as_ptr(), FILE_GENERIC_READ | FILE_GENERIC_WRITE, 0, null_mut(), OPEN_EXISTING, 0, null_mut())
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(format!("could not open Bridge Display Service pipe: {}", io::Error::last_os_error()));
    }
    let pipe = OwnedPipe(handle);
    let request = format!("BRIDGE_DISPLAY/1 {command}");
    let mut written = 0_u32;
    let write_ok = unsafe { WriteFile(pipe.0, request.as_ptr(), request.len() as u32, &mut written, null_mut()) };
    if write_ok == 0 || written as usize != request.len() {
        return Err(format!("could not write display-service request: {}", io::Error::last_os_error()));
    }
    let mut response = vec![0_u8; response_capacity];
    let mut read = 0_u32;
    let read_ok = unsafe { ReadFile(pipe.0, response.as_mut_ptr(), response.len() as u32, &mut read, null_mut()) };
    if read_ok == 0 {
        return Err(format!("could not read display-service response: {}", io::Error::last_os_error()));
    }
    if read == 0 || read as usize > response_capacity {
        return Err("display service returned an empty or oversized response".into());
    }
    response.truncate(read as usize);
    Ok(response)
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
    fn parses_only_valid_upstream_vdd_topology() {
        assert_eq!(
            parse_vdd_topology("BRIDGE_DISPLAY/1 OK vdd_present=1 attached=1\n").unwrap(),
            VddTopologyStatus { present: true, attached_to_desktop: true }
        );
        assert_eq!(
            parse_vdd_topology("BRIDGE_DISPLAY/1 OK vdd_present=0 attached=0\n").unwrap(),
            VddTopologyStatus { present: false, attached_to_desktop: false }
        );
        assert!(parse_vdd_topology("BRIDGE_DISPLAY/1 OK vdd_present=2 attached=1").is_err());
        assert!(parse_vdd_topology("BRIDGE_DISPLAY/1 OK vdd_present=1 attached=2").is_err());
        assert!(parse_vdd_topology("BRIDGE_DISPLAY/1 OK vdd_present=0 attached=1").is_err());
        assert!(parse_vdd_topology("BRIDGE_DISPLAY/1 ERROR display adapter absent").is_err());
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
