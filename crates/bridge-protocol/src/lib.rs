//! Versioned wire types shared by Bridge clients.
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DeviceId(pub Uuid);

impl DeviceId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for DeviceId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Windows,
    Android,
    Ios,
    Linux,
    Macos,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceDescriptor {
    pub id: DeviceId,
    pub display_name: String,
    pub platform: Platform,
    pub protocol_version: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CapabilityId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum MessageKind {
    Ping,
    Pong,
    CapabilityAnnouncement {
        capabilities: Vec<CapabilityId>,
    },
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
        Self {
            protocol_version: PROTOCOL_VERSION,
            message_id: Uuid::new_v4(),
            sent_at_ms,
            kind,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_round_trips_as_json() {
        let message = MessageEnvelope::new(MessageKind::Ping, 42);
        let encoded = serde_json::to_string(&message).expect("serialize");
        let decoded: MessageEnvelope = serde_json::from_str(&encoded).expect("deserialize");
        assert_eq!(decoded, message);
    }

    #[test]
    fn device_ids_are_unique() {
        assert_ne!(DeviceId::new(), DeviceId::new());
    }
}
