//! Versioned local control protocol shared by the Bridge UI and Windows service.
//!
//! The pipe is local-only. The service validates every command and keeps privileged
//! driver access out of the normal desktop process.

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_REQUEST_BYTES: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    Ping,
    Query,
    Connect,
    Disconnect,
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
            "BRIDGE_DISPLAY/1 CONNECT" => Ok(Self::Connect),
            "BRIDGE_DISPLAY/1 DISCONNECT" => Ok(Self::Disconnect),
            _ => Err("unsupported display-service request"),
        }
    }
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
        assert_eq!(Request::parse(b"BRIDGE_DISPLAY/1 CONNECT"), Ok(Request::Connect));
        assert_eq!(Request::parse(b"BRIDGE_DISPLAY/1 DISCONNECT"), Ok(Request::Disconnect));
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionLease {
    pub id: uuid::Uuid,
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

/// Single-display lease state. Issuance/renewal must only be called after the
/// service independently authenticates the Bridge desktop client and its live
/// Noise-session assertion. This registry is not an authentication mechanism
/// and is deliberately not exposed over the named pipe yet.
#[derive(Debug, Default)]
pub struct SessionLeaseRegistry {
    active: Option<SessionLease>,
}

impl SessionLeaseRegistry {
    pub fn issue_after_authentication(
        &mut self,
        remote_fingerprint: &str,
        ttl: std::time::Duration,
    ) -> Result<SessionLease, &'static str> {
        validate_fingerprint(remote_fingerprint)?;
        validate_lease_ttl(ttl)?;
        let lease = SessionLease {
            id: uuid::Uuid::new_v4(),
            remote_fingerprint: remote_fingerprint.to_ascii_lowercase(),
            expires_at: std::time::Instant::now() + ttl,
        };
        self.active = Some(lease.clone());
        Ok(lease)
    }

    pub fn renew_after_authentication(
        &mut self,
        lease_id: uuid::Uuid,
        remote_fingerprint: &str,
        ttl: std::time::Duration,
    ) -> Result<SessionLease, &'static str> {
        validate_fingerprint(remote_fingerprint)?;
        validate_lease_ttl(ttl)?;
        let current = self.active.as_ref().ok_or("no active session lease")?;
        if !current.is_valid_at(std::time::Instant::now()) {
            self.active = None;
            return Err("session lease expired");
        }
        if current.id != lease_id || !current.remote_fingerprint.eq_ignore_ascii_case(remote_fingerprint) {
            return Err("session lease identity mismatch");
        }
        let renewed = SessionLease {
            id: current.id,
            remote_fingerprint: current.remote_fingerprint.clone(),
            expires_at: std::time::Instant::now() + ttl,
        };
        self.active = Some(renewed.clone());
        Ok(renewed)
    }

    pub fn revoke(&mut self, lease_id: uuid::Uuid) -> bool {
        if self.active.as_ref().is_some_and(|lease| lease.id == lease_id) {
            self.active = None;
            true
        } else {
            false
        }
    }

    pub fn active_for(&mut self, remote_fingerprint: &str) -> Option<&SessionLease> {
        let now = std::time::Instant::now();
        if self.active.as_ref().is_some_and(|lease| !lease.is_valid_at(now)) {
            self.active = None;
        }
        self.active.as_ref().filter(|lease| lease.remote_fingerprint.eq_ignore_ascii_case(remote_fingerprint))
    }

    pub fn clear(&mut self) {
        self.active = None;
    }
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
    fn issues_single_device_lease_and_replaces_previous_owner() {
        let mut registry = SessionLeaseRegistry::default();
        let first = registry.issue_after_authentication(FINGERPRINT, MAX_SESSION_LEASE).unwrap();
        let second = registry.issue_after_authentication(&"A".repeat(64), MAX_SESSION_LEASE).unwrap();
        assert_ne!(first.id, second.id);
        assert!(registry.active_for(FINGERPRINT).is_none());
        assert!(registry.active_for(&"a".repeat(64)).is_some());
    }

    #[test]
    fn renewal_requires_matching_lease_and_peer_identity() {
        let mut registry = SessionLeaseRegistry::default();
        let lease = registry.issue_after_authentication(FINGERPRINT, MAX_SESSION_LEASE).unwrap();
        assert!(registry.renew_after_authentication(lease.id, &"f".repeat(64), MAX_SESSION_LEASE).is_err());
        assert!(registry.renew_after_authentication(uuid::Uuid::new_v4(), FINGERPRINT, MAX_SESSION_LEASE).is_err());
        assert!(registry.renew_after_authentication(lease.id, FINGERPRINT, MAX_SESSION_LEASE).is_ok());
    }

    #[test]
    fn explicit_revocation_and_clear_remove_authorization() {
        let mut registry = SessionLeaseRegistry::default();
        let lease = registry.issue_after_authentication(FINGERPRINT, MAX_SESSION_LEASE).unwrap();
        assert!(!registry.revoke(uuid::Uuid::new_v4()));
        assert!(registry.revoke(lease.id));
        assert!(registry.active_for(FINGERPRINT).is_none());
        let lease = registry.issue_after_authentication(FINGERPRINT, MAX_SESSION_LEASE).unwrap();
        registry.clear();
        assert!(registry.active_for(FINGERPRINT).is_none());
        assert!(!registry.revoke(lease.id));
    }

    #[test]
    fn rejects_invalid_fingerprints_and_unbounded_ttls() {
        let mut registry = SessionLeaseRegistry::default();
        assert!(registry.issue_after_authentication("not-a-fingerprint", MAX_SESSION_LEASE).is_err());
        assert!(registry.issue_after_authentication(FINGERPRINT, std::time::Duration::ZERO).is_err());
        assert!(registry.issue_after_authentication(FINGERPRINT, MAX_SESSION_LEASE + std::time::Duration::from_millis(1)).is_err());
    }

    #[test]
    fn expired_lease_is_not_returned() {
        let mut registry = SessionLeaseRegistry::default();
        let lease = registry.issue_after_authentication(FINGERPRINT, std::time::Duration::from_millis(1)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        assert!(registry.active_for(FINGERPRINT).is_none());
        assert!(!registry.revoke(lease.id));
    }
}
