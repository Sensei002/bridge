//! Explicit-consent pairing state machine.
//!
//! This module manages user approval and expiry only. It does not authenticate
//! network peers or establish encryption; callers must not expose it over an
//! unauthenticated transport.
use std::collections::{HashMap, HashSet};
use bridge_protocol::{DeviceDescriptor, DeviceId, PairingRequest, ProtocolValidationError, MAX_PAIRING_WINDOW_MS};
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


/// QR invitations are short-lived endpoint hints, never authentication credentials.
/// The peer fingerprint is supplied as the expected Noise XX identity during
/// connection; users must still compare the SAS and approve explicitly.
pub const QR_INVITATION_TTL_MS: u64 = 120_000;
const QR_INVITATION_MAX_BYTES: usize = 2_048;
const QR_INVITATION_CLOCK_SKEW_MS: u64 = 15_000;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct QrInvitation {
    pub version: u8,
    pub device_id: DeviceId,
    pub display_name: String,
    pub host: String,
    pub port: u16,
    pub fingerprint: String,
    pub nonce: Uuid,
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum QrInvitationError {
    #[error("QR invitation payload is too large")]
    TooLarge,
    #[error("QR invitation payload is invalid")]
    InvalidPayload,
    #[error("QR invitation version is unsupported")]
    UnsupportedVersion,
    #[error("QR invitation has expired or its clock is invalid")]
    Expired,
    #[error("QR invitation endpoint is invalid")]
    InvalidEndpoint,
    #[error("QR invitation fingerprint is invalid")]
    InvalidFingerprint,
    #[error("QR invitation has already been consumed")]
    Replayed,
    #[error("QR invitation replay cache is full")]
    ReplayCacheFull,
}

impl QrInvitation {
    pub fn create(
        device: &DeviceDescriptor,
        host: &str,
        port: u16,
        fingerprint: &str,
        now_ms: u64,
    ) -> Result<Self, QrInvitationError> {
        device.validate().map_err(|_| QrInvitationError::InvalidPayload)?;
        let invitation = Self {
            version: 1,
            device_id: device.id,
            display_name: device.display_name.clone(),
            host: host.to_owned(),
            port,
            fingerprint: fingerprint.to_ascii_lowercase(),
            nonce: Uuid::new_v4(),
            created_at_ms: now_ms,
            expires_at_ms: now_ms.saturating_add(QR_INVITATION_TTL_MS),
        };
        invitation.validate(now_ms)?;
        Ok(invitation)
    }

    pub fn encode(&self) -> Result<String, QrInvitationError> {
        let payload = serde_json::to_string(self).map_err(|_| QrInvitationError::InvalidPayload)?;
        if payload.len() > QR_INVITATION_MAX_BYTES { return Err(QrInvitationError::TooLarge); }
        Ok(format!("bridge://pair/{}", payload))
    }

    pub fn decode(uri: &str, now_ms: u64) -> Result<Self, QrInvitationError> {
        let payload = uri.strip_prefix("bridge://pair/")
            .ok_or(QrInvitationError::InvalidPayload)?;
        if payload.len() > QR_INVITATION_MAX_BYTES { return Err(QrInvitationError::TooLarge); }
        let invitation: Self = serde_json::from_str(payload)
            .map_err(|_| QrInvitationError::InvalidPayload)?;
        invitation.validate(now_ms)?;
        Ok(invitation)
    }

    pub fn validate(&self, now_ms: u64) -> Result<(), QrInvitationError> {
        if self.version != 1 { return Err(QrInvitationError::UnsupportedVersion); }
        if self.display_name.trim().is_empty() || self.display_name.len() > 64 {
            return Err(QrInvitationError::InvalidPayload);
        }
        if self.port == 0 || self.host.parse::<std::net::IpAddr>().is_err() {
            return Err(QrInvitationError::InvalidEndpoint);
        }
        if self.fingerprint.len() != 64
            || !self.fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(QrInvitationError::InvalidFingerprint);
        }
        if self.expires_at_ms <= self.created_at_ms
            || self.expires_at_ms - self.created_at_ms > QR_INVITATION_TTL_MS
            || self.created_at_ms > now_ms.saturating_add(QR_INVITATION_CLOCK_SKEW_MS)
            || now_ms >= self.expires_at_ms {
            return Err(QrInvitationError::Expired);
        }
        Ok(())
    }
}

/// Process-local, bounded one-time nonce guard for invitations accepted by a client.
/// Call only after decoding and validating the invitation, immediately before connecting.
#[derive(Debug, Default)]
pub struct QrInvitationReplayGuard {
    consumed: HashSet<Uuid>,
    expirations: HashMap<Uuid, u64>,
}

impl QrInvitationReplayGuard {
    pub const MAX_TRACKED_NONCES: usize = 4_096;

    pub fn consume(&mut self, invitation: &QrInvitation, now_ms: u64) -> Result<(), QrInvitationError> {
        invitation.validate(now_ms)?;
        self.expirations.retain(|_, expires_at| *expires_at > now_ms);
        self.consumed.retain(|nonce| self.expirations.contains_key(nonce));
        if self.consumed.contains(&invitation.nonce) {
            return Err(QrInvitationError::Replayed);
        }
        if self.consumed.len() >= Self::MAX_TRACKED_NONCES {
            return Err(QrInvitationError::ReplayCacheFull);
        }
        self.consumed.insert(invitation.nonce);
        self.expirations.insert(invitation.nonce, invitation.expires_at_ms);
        Ok(())
    }

    pub fn tracked_nonces(&self) -> usize {
        self.consumed.len()
    }
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
    fn qr_invitation_round_trips_and_expires() {
        let device = device();
        let invitation = QrInvitation::create(
            &device, "192.168.1.25", 45821, &"ab".repeat(32), 10_000
        ).unwrap();
        let uri = invitation.encode().unwrap();
        assert_eq!(QrInvitation::decode(&uri, 10_001).unwrap(), invitation);
        assert_eq!(
            QrInvitation::decode(&uri, 10_000 + QR_INVITATION_TTL_MS),
            Err(QrInvitationError::Expired)
        );
    }

    #[test]
    fn qr_invitation_rejects_untrusted_endpoint_and_fingerprint() {
        let device = device();
        assert_eq!(
            QrInvitation::create(&device, "attacker.example", 45821, &"ab".repeat(32), 1),
            Err(QrInvitationError::InvalidEndpoint)
        );
        assert_eq!(
            QrInvitation::create(&device, "192.168.1.25", 0, &"ab".repeat(32), 1),
            Err(QrInvitationError::InvalidEndpoint)
        );
        assert_eq!(
            QrInvitation::create(&device, "192.168.1.25", 45821, "not-a-fingerprint", 1),
            Err(QrInvitationError::InvalidFingerprint)
        );
    }

    #[test]
    fn qr_invitation_rejects_future_and_oversized_payloads() {
        let device = device();
        let invitation = QrInvitation::create(
            &device, "10.0.0.2", 45821, &"cd".repeat(32), 100_000
        ).unwrap();
        let uri = invitation.encode().unwrap();
        assert_eq!(QrInvitation::decode(&uri, 1), Err(QrInvitationError::Expired));
        assert_eq!(
            QrInvitation::decode(&format!("bridge://pair/{}", "x".repeat(QR_INVITATION_MAX_BYTES + 1)), 100_000),
            Err(QrInvitationError::TooLarge)
        );
    }

    #[test]
    fn qr_invitation_nonce_is_one_time_and_expires_from_replay_guard() {
        let device = device();
        let invitation = QrInvitation::create(
            &device, "192.168.1.25", 45821, &"ab".repeat(32), 10_000
        ).unwrap();
        let mut guard = QrInvitationReplayGuard::default();
        assert_eq!(guard.consume(&invitation, 10_001), Ok(()));
        assert_eq!(guard.consume(&invitation, 10_002), Err(QrInvitationError::Replayed));
        assert_eq!(guard.tracked_nonces(), 1);
        assert_eq!(guard.consume(&invitation, invitation.expires_at_ms), Err(QrInvitationError::Expired));
        assert_eq!(guard.tracked_nonces(), 0);
    }

    #[test]
    fn qr_invitation_replay_guard_is_bounded() {
        let device = device();
        let mut guard = QrInvitationReplayGuard::default();
        for index in 0..QrInvitationReplayGuard::MAX_TRACKED_NONCES {
            let invitation = QrInvitation::create(
                &device, "192.168.1.25", 45821, &"ab".repeat(32), 10_000
            ).unwrap();
            // Each invitation has a cryptographically random nonce.
            assert_eq!(guard.consume(&invitation, 10_001 + index as u64), Ok(()));
        }
        let final_invitation = QrInvitation::create(
            &device, "192.168.1.25", 45821, &"ab".repeat(32), 10_000
        ).unwrap();
        assert_eq!(guard.consume(&final_invitation, 10_002), Err(QrInvitationError::ReplayCacheFull));
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
