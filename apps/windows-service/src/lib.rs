//! Versioned local control protocol shared by the Bridge UI and Windows service.
//!
//! The pipe is local-only. The service validates every command and keeps privileged
//! driver access out of the normal desktop process.

pub mod challenge;

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_REQUEST_BYTES: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Ping,
    Query,
    Challenge,
    ChallengeResponse(String),
    AttestationChallenge { session_id: uuid::Uuid, device_fingerprint: String, pinned_public_key: String },
    AttestationResponse { nonce: String, session_id: uuid::Uuid, device_fingerprint: String, signature: String },
    Renew { lease_id: uuid::Uuid, device_fingerprint: String },
    Connect { lease_id: uuid::Uuid, device_fingerprint: String },
    Disconnect { lease_id: uuid::Uuid, device_fingerprint: String },
    ReadFrame { lease_id: uuid::Uuid, device_fingerprint: String },
}

impl Request {
    pub fn parse(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.is_empty() || bytes.len() > MAX_REQUEST_BYTES {
            return Err("invalid request length");
        }
        let text = std::str::from_utf8(bytes).map_err(|_| "request must be UTF-8")?;
        match text.trim() {
            "BRIDGE_DISPLAY/1 PING" => Ok(Self::Ping),
            "BRIDGE_DISPLAY/1 QUERY" => Ok(Self::Query),
            "BRIDGE_DISPLAY/1 CHALLENGE" => Ok(Self::Challenge),
            value if value.starts_with("BRIDGE_DISPLAY/1 CHALLENGE_RESPONSE ") => {
                let nonce = &value["BRIDGE_DISPLAY/1 CHALLENGE_RESPONSE ".len()..];
                if nonce.len() != 64 || !nonce.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    return Err("challenge response must be exactly 64 hexadecimal characters");
                }
                Ok(Self::ChallengeResponse(nonce.to_ascii_lowercase()))
            }
            value if value.starts_with("BRIDGE_DISPLAY/1 CHALLENGE_ATTESTATION ") => {
                let fields = value["BRIDGE_DISPLAY/1 CHALLENGE_ATTESTATION ".len()..]
                    .split_whitespace().collect::<Vec<_>>();
                if fields.len() != 3
                    || !fields[1].starts_with("fingerprint=")
                    || !fields[2].starts_with("public_key=")
                    || !fields[0].starts_with("session=")
                {
                    return Err("attestation challenge fields are malformed");
                }
                let session_id = fields[0]["session=".len()..].parse()
                    .map_err(|_| "attestation session ID is invalid")?;
                let device_fingerprint = fields[1]["fingerprint=".len()..].to_ascii_lowercase();
                let pinned_public_key = fields[2]["public_key=".len()..].to_ascii_lowercase();
                if device_fingerprint.len() != 64 || !device_fingerprint.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err("attestation fingerprint must be 64 hexadecimal characters");
                }
                if pinned_public_key.len() != 64 || !pinned_public_key.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err("pinned attestation public key must be 64 hexadecimal characters");
                }
                Ok(Self::AttestationChallenge { session_id, device_fingerprint, pinned_public_key })
            }
            value if value.starts_with("BRIDGE_DISPLAY/1 ATTESTATION_RESPONSE ") => {
                let fields = value["BRIDGE_DISPLAY/1 ATTESTATION_RESPONSE ".len()..]
                    .split_whitespace().collect::<Vec<_>>();
                if fields.len() != 4
                    || !fields[0].starts_with("nonce=")
                    || !fields[1].starts_with("session=")
                    || !fields[2].starts_with("fingerprint=")
                    || !fields[3].starts_with("signature=")
                {
                    return Err("attestation response fields are malformed");
                }
                let nonce = fields[0]["nonce=".len()..].to_ascii_lowercase();
                let session_id = fields[1]["session=".len()..].parse()
                    .map_err(|_| "attestation session ID is invalid")?;
                let device_fingerprint = fields[2]["fingerprint=".len()..].to_ascii_lowercase();
                let signature = fields[3]["signature=".len()..].to_ascii_lowercase();
                if nonce.len() != 64 || !nonce.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err("attestation nonce must be 64 hexadecimal characters");
                }
                if device_fingerprint.len() != 64 || !device_fingerprint.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err("attestation fingerprint must be 64 hexadecimal characters");
                }
                if signature.len() != 128 || !signature.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err("attestation signature must be 128 hexadecimal characters");
                }
                Ok(Self::AttestationResponse { nonce, session_id, device_fingerprint, signature })
            }
            value if value.starts_with("BRIDGE_DISPLAY/1 RENEW ") => {
                parse_lease_command(value, "BRIDGE_DISPLAY/1 RENEW ")
                    .map(|(lease_id, device_fingerprint)| Self::Renew { lease_id, device_fingerprint })
            }
            value if value.starts_with("BRIDGE_DISPLAY/1 CONNECT ") => {
                parse_lease_command(value, "BRIDGE_DISPLAY/1 CONNECT ")
                    .map(|(lease_id, device_fingerprint)| Self::Connect { lease_id, device_fingerprint })
            }
            value if value.starts_with("BRIDGE_DISPLAY/1 DISCONNECT ") => {
                parse_lease_command(value, "BRIDGE_DISPLAY/1 DISCONNECT ")
                    .map(|(lease_id, device_fingerprint)| Self::Disconnect { lease_id, device_fingerprint })
            }
            value if value.starts_with("BRIDGE_DISPLAY/1 READ_FRAME ") => {
                parse_lease_command(value, "BRIDGE_DISPLAY/1 READ_FRAME ")
                    .map(|(lease_id, device_fingerprint)| Self::ReadFrame { lease_id, device_fingerprint })
            },
            _ => Err("unsupported display-service request"),
        }
    }
}


/// Parse the exact lease identity required for every driver mutation.
fn parse_lease_command(value: &str, prefix: &str) -> Result<(uuid::Uuid, String), &'static str> {
    let fields = value[prefix.len()..].split_whitespace().collect::<Vec<_>>();
    if fields.len() != 2 || !fields[0].starts_with("lease=") || !fields[1].starts_with("fingerprint=") {
        return Err("display mutation requires lease ID and device fingerprint");
    }
    let lease_id = fields[0]["lease=".len()..].parse()
        .map_err(|_| "display lease ID is invalid")?;
    let fingerprint = fields[1]["fingerprint=".len()..].to_ascii_lowercase();
    if fingerprint.len() != 64 || !fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("display lease fingerprint must be 64 hexadecimal characters");
    }
    Ok((lease_id, fingerprint))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayStatus {
    pub connected: bool,
    pub supported_mode_count: u32,
}

impl DisplayStatus {
    pub fn encode(self) -> String {
        format!(
            "BRIDGE_DISPLAY/1 OK connected={} modes={}\n",
            u8::from(self.connected),
            self.supported_mode_count
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_versioned_commands() {
        assert_eq!(Request::parse(b"BRIDGE_DISPLAY/1 PING"), Ok(Request::Ping));
        assert_eq!(Request::parse(b"BRIDGE_DISPLAY/1 QUERY"), Ok(Request::Query));
        assert_eq!(Request::parse(b"BRIDGE_DISPLAY/1 CHALLENGE"), Ok(Request::Challenge));
        let nonce = "ab".repeat(32);
        assert_eq!(Request::parse(format!("BRIDGE_DISPLAY/1 CHALLENGE_RESPONSE {nonce}").as_bytes()), Ok(Request::ChallengeResponse(nonce)));
        assert!(Request::parse(b"BRIDGE_DISPLAY/1 CHALLENGE_RESPONSE invalid").is_err());
        let fingerprint = "ab".repeat(32);
        let public_key = "cd".repeat(32);
        let session = uuid::Uuid::new_v4();
        let challenge = format!("BRIDGE_DISPLAY/1 CHALLENGE_ATTESTATION session={session} fingerprint={fingerprint} public_key={public_key}");
        assert!(matches!(Request::parse(challenge.as_bytes()), Ok(Request::AttestationChallenge { .. })));
        let signature = "ef".repeat(64);
        let response = format!("BRIDGE_DISPLAY/1 ATTESTATION_RESPONSE nonce={} session={session} fingerprint={fingerprint} signature={signature}", "01".repeat(32));
        assert!(matches!(Request::parse(response.as_bytes()), Ok(Request::AttestationResponse { .. })));
        assert!(Request::parse(b"BRIDGE_DISPLAY/1 ATTESTATION_RESPONSE nonce=bad").is_err());
        let lease_id = uuid::Uuid::new_v4();
        let renew_command = format!("BRIDGE_DISPLAY/1 RENEW lease={lease_id} fingerprint={fingerprint}");
        assert_eq!(
            Request::parse(renew_command.as_bytes()),
            Ok(Request::Renew { lease_id, device_fingerprint: fingerprint.clone() })
        );
        assert!(Request::parse(b"BRIDGE_DISPLAY/1 RENEW").is_err());
        let lease_command = format!("BRIDGE_DISPLAY/1 CONNECT lease={lease_id} fingerprint={fingerprint}");
        assert_eq!(
            Request::parse(lease_command.as_bytes()),
            Ok(Request::Connect { lease_id, device_fingerprint: fingerprint.clone() })
        );
        let disconnect_command = format!("BRIDGE_DISPLAY/1 DISCONNECT lease={lease_id} fingerprint={fingerprint}");
        assert_eq!(
            Request::parse(disconnect_command.as_bytes()),
            Ok(Request::Disconnect { lease_id, device_fingerprint: fingerprint.clone() })
        );
        assert!(Request::parse(b"BRIDGE_DISPLAY/1 CONNECT").is_err());
        assert!(Request::parse(format!("BRIDGE_DISPLAY/1 DISCONNECT lease={lease_id} fingerprint=bad").as_bytes()).is_err());
        let frame_command = format!("BRIDGE_DISPLAY/1 READ_FRAME lease={lease_id} fingerprint={fingerprint}");
        assert_eq!(Request::parse(frame_command.as_bytes()), Ok(Request::ReadFrame { lease_id, device_fingerprint: fingerprint.clone() }));
        assert!(Request::parse(b"CONNECT").is_err());
        assert!(Request::parse(b"BRIDGE_DISPLAY/2 QUERY").is_err());
    }

    #[test]
    fn rejects_empty_oversized_and_non_utf8_requests() {
        assert!(Request::parse(b"").is_err());
        assert!(Request::parse(&vec![b'X'; MAX_REQUEST_BYTES + 1]).is_err());
        assert!(Request::parse(&[0xff, 0xfe]).is_err());
    }

    #[test]
    fn status_response_is_versioned_and_machine_readable() {
        assert_eq!(
            DisplayStatus { connected: true, supported_mode_count: 3 }.encode(),
            "BRIDGE_DISPLAY/1 OK connected=1 modes=3\n"
        );
    }
}


/// Maximum time a desktop-session authorization lease may remain valid without renewal.
pub const MAX_SESSION_LEASE: std::time::Duration = std::time::Duration::from_secs(15);

/// A short-lived authorization lease bound to both the originating Windows
/// process and the authenticated remote device fingerprint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionLease {
    pub id: uuid::Uuid,
    pub process_id: u32,
    pub remote_fingerprint: String,
    expires_at: std::time::Instant,
}

impl SessionLease {
    pub fn remaining(&self, now: std::time::Instant) -> std::time::Duration {
        self.expires_at.saturating_duration_since(now)
    }

    pub fn is_valid_at(&self, now: std::time::Instant) -> bool {
        now < self.expires_at
    }
}

/// Single-display lease state. Callers must authenticate the signed desktop
/// process and verify a cryptographic assertion from its live Noise peer before
/// invoking issue/renew. This registry does not perform authentication itself.
#[derive(Debug, Default)]
pub struct SessionLeaseRegistry {
    active: Option<SessionLease>,
}

impl SessionLeaseRegistry {
    pub fn issue_after_authentication(
        &mut self,
        process_id: u32,
        remote_fingerprint: &str,
        ttl: std::time::Duration,
    ) -> Result<SessionLease, &'static str> {
        validate_process_id(process_id)?;
        validate_fingerprint(remote_fingerprint)?;
        validate_lease_ttl(ttl)?;
        let lease = SessionLease {
            id: uuid::Uuid::new_v4(),
            process_id,
            remote_fingerprint: remote_fingerprint.to_ascii_lowercase(),
            expires_at: std::time::Instant::now() + ttl,
        };
        self.active = Some(lease.clone());
        Ok(lease)
    }

    pub fn renew_after_authentication(
        &mut self,
        lease_id: uuid::Uuid,
        process_id: u32,
        remote_fingerprint: &str,
        ttl: std::time::Duration,
    ) -> Result<SessionLease, &'static str> {
        validate_process_id(process_id)?;
        validate_fingerprint(remote_fingerprint)?;
        validate_lease_ttl(ttl)?;
        let current = self.active.as_ref().ok_or("no active session lease")?;
        if !current.is_valid_at(std::time::Instant::now()) {
            self.active = None;
            return Err("session lease expired");
        }
        if current.id != lease_id
            || current.process_id != process_id
            || !current.remote_fingerprint.eq_ignore_ascii_case(remote_fingerprint)
        {
            return Err("session lease identity mismatch");
        }
        let renewed = SessionLease {
            id: current.id,
            process_id: current.process_id,
            remote_fingerprint: current.remote_fingerprint.clone(),
            expires_at: std::time::Instant::now() + ttl,
        };
        self.active = Some(renewed.clone());
        Ok(renewed)
    }

    /// Validate the complete lease identity immediately before a privileged driver operation.
    pub fn authorize_operation(
        &mut self,
        lease_id: uuid::Uuid,
        process_id: u32,
        remote_fingerprint: &str,
    ) -> Result<(), &'static str> {
        let lease = self.active_for(process_id, remote_fingerprint)
            .ok_or("no active display lease for this process and device")?;
        if lease.id != lease_id {
            return Err("display lease ID does not match the active lease");
        }
        Ok(())
    }

    /// Revoke only when the exact process that owns the lease is being removed.
    pub fn revoke_process(&mut self, process_id: u32) -> bool {
        if self.active.as_ref().is_some_and(|lease| lease.process_id == process_id) {
            self.active = None;
            true
        } else {
            false
        }
    }

    pub fn revoke(&mut self, lease_id: uuid::Uuid) -> bool {
        if self.active.as_ref().is_some_and(|lease| lease.id == lease_id) {
            self.active = None;
            true
        } else {
            false
        }
    }

    pub fn active_for(
        &mut self,
        process_id: u32,
        remote_fingerprint: &str,
    ) -> Option<&SessionLease> {
        let now = std::time::Instant::now();
        if self.active.as_ref().is_some_and(|lease| !lease.is_valid_at(now)) {
            self.active = None;
        }
        self.active.as_ref().filter(|lease| {
            lease.process_id == process_id
                && lease.remote_fingerprint.eq_ignore_ascii_case(remote_fingerprint)
        })
    }

    pub fn clear(&mut self) {
        self.active = None;
    }
}

fn validate_process_id(process_id: u32) -> Result<(), &'static str> {
    if process_id == 0 {
        return Err("process ID must be nonzero");
    }
    Ok(())
}

fn validate_fingerprint(value: &str) -> Result<(), &'static str> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("remote fingerprint must be 64 hexadecimal characters");
    }
    Ok(())
}

fn validate_lease_ttl(ttl: std::time::Duration) -> Result<(), &'static str> {
    if ttl.is_zero() || ttl > MAX_SESSION_LEASE {
        return Err("session lease TTL must be between 1ms and 15s");
    }
    Ok(())
}

#[cfg(test)]
mod lease_tests {
    use super::*;
    const FINGERPRINT: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn issues_single_process_and_peer_bound_lease_and_replaces_previous_owner() {
        let mut registry = SessionLeaseRegistry::default();
        let first = registry.issue_after_authentication(42, FINGERPRINT, MAX_SESSION_LEASE).unwrap();
        let second = registry.issue_after_authentication(43, &"A".repeat(64), MAX_SESSION_LEASE).unwrap();
        assert_ne!(first.id, second.id);
        assert!(registry.active_for(42, FINGERPRINT).is_none());
        assert!(registry.active_for(43, &"a".repeat(64)).is_some());
    }

    #[test]
    fn renewal_requires_matching_lease_process_and_peer_identity() {
        let mut registry = SessionLeaseRegistry::default();
        let lease = registry.issue_after_authentication(42, FINGERPRINT, MAX_SESSION_LEASE).unwrap();
        assert!(registry.renew_after_authentication(lease.id, 42, &"f".repeat(64), MAX_SESSION_LEASE).is_err());
        assert!(registry.renew_after_authentication(uuid::Uuid::new_v4(), 42, FINGERPRINT, MAX_SESSION_LEASE).is_err());
        assert!(registry.renew_after_authentication(lease.id, 43, FINGERPRINT, MAX_SESSION_LEASE).is_err());
        assert!(registry.renew_after_authentication(lease.id, 42, FINGERPRINT, MAX_SESSION_LEASE).is_ok());
    }

    #[test]
    fn operation_authorization_requires_exact_lease_process_and_peer() {
        let mut registry = SessionLeaseRegistry::default();
        let lease = registry.issue_after_authentication(42, FINGERPRINT, MAX_SESSION_LEASE).unwrap();
        assert!(registry.authorize_operation(lease.id, 42, FINGERPRINT).is_ok());
        assert!(registry.authorize_operation(uuid::Uuid::new_v4(), 42, FINGERPRINT).is_err());
        assert!(registry.authorize_operation(lease.id, 43, FINGERPRINT).is_err());
        assert!(registry.authorize_operation(lease.id, 42, &"f".repeat(64)).is_err());
        registry.revoke(lease.id);
        assert!(registry.authorize_operation(lease.id, 42, FINGERPRINT).is_err());
    }

    #[test]
    fn process_revocation_cannot_revoke_another_process_lease() {
        let mut registry = SessionLeaseRegistry::default();
        registry.issue_after_authentication(42, FINGERPRINT, MAX_SESSION_LEASE).unwrap();
        assert!(!registry.revoke_process(43));
        assert!(registry.active_for(42, FINGERPRINT).is_some());
        assert!(registry.revoke_process(42));
        assert!(registry.active_for(42, FINGERPRINT).is_none());
    }

    #[test]
    fn explicit_revocation_and_clear_remove_authorization() {
        let mut registry = SessionLeaseRegistry::default();
        let lease = registry.issue_after_authentication(42, FINGERPRINT, MAX_SESSION_LEASE).unwrap();
        assert!(!registry.revoke(uuid::Uuid::new_v4()));
        assert!(registry.revoke(lease.id));
        assert!(registry.active_for(42, FINGERPRINT).is_none());
        let lease = registry.issue_after_authentication(42, FINGERPRINT, MAX_SESSION_LEASE).unwrap();
        registry.clear();
        assert!(registry.active_for(42, FINGERPRINT).is_none());
        assert!(!registry.revoke(lease.id));
    }

    #[test]
    fn rejects_invalid_process_ids_fingerprints_and_unbounded_ttls() {
        let mut registry = SessionLeaseRegistry::default();
        assert!(registry.issue_after_authentication(0, FINGERPRINT, MAX_SESSION_LEASE).is_err());
        assert!(registry.issue_after_authentication(42, "not-a-fingerprint", MAX_SESSION_LEASE).is_err());
        assert!(registry.issue_after_authentication(42, FINGERPRINT, std::time::Duration::ZERO).is_err());
        assert!(registry.issue_after_authentication(42, FINGERPRINT, MAX_SESSION_LEASE + std::time::Duration::from_millis(1)).is_err());
    }

    #[test]
    fn expired_lease_is_not_returned() {
        let mut registry = SessionLeaseRegistry::default();
        let lease = registry.issue_after_authentication(42, FINGERPRINT, std::time::Duration::from_millis(1)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        assert!(registry.active_for(42, FINGERPRINT).is_none());
        assert!(!registry.revoke(lease.id));
    }
}
