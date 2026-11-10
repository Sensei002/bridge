//! Versioned wire types shared by Bridge clients.
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PROTOCOL_VERSION: u16 = 1;
pub const MAX_DEVICE_NAME_BYTES: usize = 64;
pub const MAX_PAIRING_WINDOW_MS: u64 = 120_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DeviceId(pub Uuid);

impl DeviceId {
    pub fn new() -> Self { Self(Uuid::new_v4()) }
}
impl Default for DeviceId {
    fn default() -> Self { Self::new() }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform { Windows, Android, Ios, Linux, Macos, Unknown }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceDescriptor {
    pub id: DeviceId,
    pub display_name: String,
    pub platform: Platform,
    pub protocol_version: u16,
    /// Optional Ed25519 attestation public key advertised inside the authenticated Noise session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attestation_public_key: Option<String>,
}

impl DeviceDescriptor {
    pub fn validate(&self) -> Result<(), ProtocolValidationError> {
        if self.display_name.trim().is_empty() { return Err(ProtocolValidationError::EmptyDeviceName); }
        if self.display_name.len() > MAX_DEVICE_NAME_BYTES { return Err(ProtocolValidationError::DeviceNameTooLong); }
        if self.protocol_version != PROTOCOL_VERSION { return Err(ProtocolValidationError::UnsupportedVersion(self.protocol_version)); }
        if let Some(key) = &self.attestation_public_key {
            if key.len() != 64 || !key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(ProtocolValidationError::InvalidAttestationPublicKey);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProtocolValidationError {
    #[error("device name cannot be empty")] EmptyDeviceName,
    #[error("device name exceeds {MAX_DEVICE_NAME_BYTES} UTF-8 bytes")] DeviceNameTooLong,
    #[error("unsupported protocol version: {0}")] UnsupportedVersion(u16),
    #[error("pairing request has an invalid expiry window")] InvalidPairingWindow,
    #[error("attestation public key must be 64 hexadecimal characters")] InvalidAttestationPublicKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CapabilityId(pub String);

/// A consent request. This message is not authenticated by itself; it must only
/// be accepted over an authenticated transport in a future protocol milestone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairingRequest {
    pub request_id: Uuid,
    pub initiator: DeviceDescriptor,
    pub nonce: Uuid,
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
}

impl PairingRequest {
    pub fn validate(&self, now_ms: u64) -> Result<(), ProtocolValidationError> {
        self.initiator.validate()?;
        let window = self.expires_at_ms.saturating_sub(self.created_at_ms);
        if window == 0 || window > MAX_PAIRING_WINDOW_MS || now_ms > self.expires_at_ms {
            return Err(ProtocolValidationError::InvalidPairingWindow);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PairingDecisionKind { Approved, Rejected }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum MessageKind {
    Ping,
    Pong,
    CapabilityAnnouncement { capabilities: Vec<CapabilityId> },
    PairingRequest(PairingRequest),
    PairingDecision { request_id: Uuid, decision: PairingDecisionKind },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageEnvelope {
    pub protocol_version: u16,
    pub message_id: Uuid,
    pub sent_at_ms: u64,
    pub kind: MessageKind,
}

impl MessageEnvelope {
    pub fn new(kind: MessageKind, sent_at_ms: u64) -> Self {
        Self { protocol_version: PROTOCOL_VERSION, message_id: Uuid::new_v4(), sent_at_ms, kind }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device() -> DeviceDescriptor {
        DeviceDescriptor { id: DeviceId::new(), display_name: "Bridge test".into(), platform: Platform::Windows, protocol_version: PROTOCOL_VERSION, attestation_public_key: None }
    }

    #[test]
    fn envelope_round_trips_as_json() {
        let message = MessageEnvelope::new(MessageKind::Ping, 42);
        let encoded = serde_json::to_string(&message).expect("serialize");
        let decoded: MessageEnvelope = serde_json::from_str(&encoded).expect("deserialize");
        assert_eq!(decoded, message);
    }

    #[test]
    fn descriptor_accepts_only_well_formed_attestation_public_keys() {
        let mut descriptor = device();
        descriptor.attestation_public_key = Some("ab".repeat(32));
        assert!(descriptor.validate().is_ok());
        descriptor.attestation_public_key = Some("not-a-key".into());
        assert_eq!(descriptor.validate(), Err(ProtocolValidationError::InvalidAttestationPublicKey));
    }

    #[test]
    fn device_ids_are_unique() { assert_ne!(DeviceId::new(), DeviceId::new()); }

    #[test]
    fn pairing_request_rejects_expired_or_overlong_windows() {
        let request = PairingRequest { request_id: Uuid::new_v4(), initiator: device(), nonce: Uuid::new_v4(), created_at_ms: 10, expires_at_ms: 20 };
        assert!(request.validate(21).is_err());
        let mut long = request;
        long.expires_at_ms = 10 + MAX_PAIRING_WINDOW_MS + 1;
        assert!(long.validate(11).is_err());
    }
}
