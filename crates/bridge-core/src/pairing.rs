//! Explicit-consent pairing state machine.
//!
//! This module manages user approval and expiry only. It does not authenticate
//! network peers or establish encryption; callers must not expose it over an
//! unauthenticated transport.
use std::collections::HashMap;
use bridge_protocol::{DeviceDescriptor, DeviceId, PairingRequest, ProtocolValidationError, MAX_PAIRING_WINDOW_MS, PROTOCOL_VERSION};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PairingError {
    #[error("pairing request is invalid: {0}")]
    InvalidRequest(String),
    #[error("pairing request is already pending")]
    AlreadyPending,
    #[error("pairing request was not found or has expired")]
    NotFound,
    #[error("a device cannot pair with itself")]
    SelfPairing,
    #[error("device is already trusted")]
    AlreadyTrusted,
}

impl From<ProtocolValidationError> for PairingError {
    fn from(value: ProtocolValidationError) -> Self { Self::InvalidRequest(value.to_string()) }
}

#[derive(Debug, Default)]
pub struct PairingCoordinator {
    local_device_id: Option<DeviceId>,
    pending: HashMap<Uuid, PairingRequest>,
    trusted: HashMap<DeviceId, DeviceDescriptor>,
}

impl PairingCoordinator {
    pub fn new(local_device_id: DeviceId) -> Self {
        Self { local_device_id: Some(local_device_id), pending: HashMap::new(), trusted: HashMap::new() }
    }

    /// Queue a remote request for explicit local user approval.
    pub fn receive_request(&mut self, request: PairingRequest, now_ms: u64) -> Result<(), PairingError> {
        request.validate(now_ms)?;
        if self.local_device_id == Some(request.initiator.id) { return Err(PairingError::SelfPairing); }
        if self.trusted.contains_key(&request.initiator.id) { return Err(PairingError::AlreadyTrusted); }
        if self.pending.contains_key(&request.request_id) { return Err(PairingError::AlreadyPending); }
        self.pending.insert(request.request_id, request);
        Ok(())
    }

    /// Approve a pending request. The caller must obtain an explicit user action first.
    pub fn approve(&mut self, request_id: Uuid, now_ms: u64) -> Result<DeviceDescriptor, PairingError> {
        let request = self.pending.remove(&request_id).ok_or(PairingError::NotFound)?;
        if now_ms > request.expires_at_ms { return Err(PairingError::NotFound); }
        let device = request.initiator;
        self.trusted.insert(device.id, device.clone());
        Ok(device)
    }

    pub fn reject(&mut self, request_id: Uuid) -> Result<(), PairingError> {
        self.pending.remove(&request_id).map(|_| ()).ok_or(PairingError::NotFound)
    }

    pub fn expire(&mut self, now_ms: u64) -> usize {
        let before = self.pending.len();
        self.pending.retain(|_, request| request.expires_at_ms >= now_ms);
        before - self.pending.len()
    }

    pub fn pending_requests(&self) -> Vec<&PairingRequest> { self.pending.values().collect() }
    pub fn trusted_devices(&self) -> Vec<&DeviceDescriptor> { self.trusted.values().collect() }
    pub fn is_trusted(&self, id: &DeviceId) -> bool { self.trusted.contains_key(id) }
}

pub fn create_pairing_request(initiator: DeviceDescriptor, now_ms: u64) -> Result<PairingRequest, PairingError> {
    initiator.validate()?;
    Ok(PairingRequest {
        request_id: Uuid::new_v4(),
        nonce: Uuid::new_v4(),
        initiator,
        created_at_ms: now_ms,
        expires_at_ms: now_ms.saturating_add(MAX_PAIRING_WINDOW_MS),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bridge_protocol::{Platform, PROTOCOL_VERSION};

    fn device() -> DeviceDescriptor {
        DeviceDescriptor { id: DeviceId::new(), display_name: "Phone".into(), platform: Platform::Android, protocol_version: PROTOCOL_VERSION }
    }

    #[test]
    fn pairing_requires_explicit_approval() {
        let local = DeviceId::new();
        let remote = device();
        let remote_id = remote.id;
        let request = create_pairing_request(remote, 100).unwrap();
        let request_id = request.request_id;
        let mut coordinator = PairingCoordinator::new(local);
        coordinator.receive_request(request, 101).unwrap();
        assert!(!coordinator.is_trusted(&remote_id));
        let approved = coordinator.approve(request_id, 102).unwrap();
        assert_eq!(approved.id, remote_id);
        assert!(coordinator.is_trusted(&remote_id));
    }

    #[test]
    fn expired_requests_are_removed() {
        let mut coordinator = PairingCoordinator::new(DeviceId::new());
        let request = create_pairing_request(device(), 100).unwrap();
        coordinator.receive_request(request, 101).unwrap();
        assert_eq!(coordinator.expire(100 + MAX_PAIRING_WINDOW_MS + 1), 1);
        assert!(coordinator.pending_requests().is_empty());
    }

    #[test]
    fn self_pairing_is_rejected() {
        let local = device();
        let request = create_pairing_request(local.clone(), 1).unwrap();
        let mut coordinator = PairingCoordinator::new(local.id);
        assert_eq!(coordinator.receive_request(request, 2), Err(PairingError::SelfPairing));
    }
}
