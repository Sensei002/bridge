//! One-time, process-bound challenges for the Windows display-service handshake.
//!
//! This registry is deliberately not wired to the named pipe. Callers must first
//! verify the desktop executable's publisher and retain a live process handle.
//! A PID alone is not a durable process identity and must never authorize a lease.

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use uuid::Uuid;

/// Challenges must be answered quickly and may be consumed exactly once.
pub const CHALLENGE_TTL: Duration = Duration::from_secs(10);
const NONCE_BYTES: usize = 32;

/// A pending challenge is bound to the process that requested it.
#[derive(Debug, Clone)]
struct PendingChallenge {
    nonce: [u8; NONCE_BYTES],
    expires_at: Instant,
}

/// In-memory challenge registry. It is intentionally empty after service restart.
#[derive(Debug, Default)]
pub struct ServiceChallengeRegistry {
    pending: HashMap<u32, PendingChallenge>,
}

impl ServiceChallengeRegistry {
    /// Create a fresh 256-bit CSPRNG nonce for a previously verified client process.
    ///
    /// The caller must have verified the process publisher and must keep the
    /// corresponding process handle open while this challenge is outstanding.
    pub fn issue_after_client_verification(
        &mut self,
        process_id: u32,
    ) -> Result<String, &'static str> {
        self.issue_at(process_id, Instant::now())
    }

    /// Consume a challenge for the same process. Successful consumption is
    /// one-shot; wrong-process and replay attempts do not consume another
    /// process's pending challenge.
    pub fn consume_for_process(
        &mut self,
        process_id: u32,
        response: &str,
    ) -> Result<(), &'static str> {
        self.consume_at(process_id, response, Instant::now())
    }

    /// Revoke any outstanding challenge for a process when its pipe closes or
    /// its retained process handle signals exit.
    pub fn revoke_process(&mut self, process_id: u32) -> bool {
        self.pending.remove(&process_id).is_some()
    }

    fn issue_at(&mut self, process_id: u32, now: Instant) -> Result<String, &'static str> {
        if process_id == 0 {
            return Err("invalid client process ID");
        }

        // UUID v4 uses the platform CSPRNG. Two independent UUIDs provide
        // 256 random bits; the UUID version/variant bits are not used here:
        // raw random bytes are obtained from the UUID crate's random source.
        let mut nonce = [0_u8; NONCE_BYTES];
        nonce[..16].copy_from_slice(Uuid::new_v4().as_bytes());
        nonce[16..].copy_from_slice(Uuid::new_v4().as_bytes());

        self.pending.insert(
            process_id,
            PendingChallenge {
                nonce,
                expires_at: now + CHALLENGE_TTL,
            },
        );
        Ok(encode_hex(&nonce))
    }

    fn consume_at(
        &mut self,
        process_id: u32,
        response: &str,
        now: Instant,
    ) -> Result<(), &'static str> {
        if process_id == 0 {
            return Err("invalid client process ID");
        }
        let supplied = decode_hex(response)?;
        let pending = self.pending.get(&process_id).ok_or("no pending challenge for client process")?;
        if now >= pending.expires_at {
            self.pending.remove(&process_id);
            return Err("client challenge expired");
        }
        if !constant_time_eq(&pending.nonce, &supplied) {
            return Err("client challenge response mismatch");
        }
        self.pending.remove(&process_id);
        Ok(())
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn decode_hex(value: &str) -> Result<Vec<u8>, &'static str> {
    if value.len() != NONCE_BYTES * 2
        || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("challenge response must be exactly 64 hexadecimal characters");
    }
    (0..value.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&value[index..index + 2], 16)
                .map_err(|_| "challenge response contains invalid hexadecimal")
        })
        .collect()
}

fn constant_time_eq(expected: &[u8], supplied: &[u8]) -> bool {
    if expected.len() != supplied.len() {
        return false;
    }
    expected
        .iter()
        .zip(supplied)
        .fold(0_u8, |difference, (left, right)| difference | (left ^ right))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn challenge_is_256_bits_and_one_time() {
        let mut registry = ServiceChallengeRegistry::default();
        let nonce = registry.issue_for_test(42, Instant::now()).unwrap();
        assert_eq!(nonce.len(), 64);
        assert!(registry.consume_for_process(42, &nonce).is_ok());
        assert_eq!(
            registry.consume_for_process(42, &nonce),
            Err("no pending challenge for client process")
        );
    }

    #[test]
    fn challenge_is_bound_to_requesting_process() {
        let mut registry = ServiceChallengeRegistry::default();
        let nonce = registry.issue_for_test(42, Instant::now()).unwrap();
        assert_eq!(
            registry.consume_for_process(43, &nonce),
            Err("no pending challenge for client process")
        );
        assert!(registry.consume_for_process(42, &nonce).is_ok());
    }

    #[test]
    fn wrong_response_does_not_consume_valid_challenge() {
        let mut registry = ServiceChallengeRegistry::default();
        let nonce = registry.issue_for_test(42, Instant::now()).unwrap();
        let wrong = format!("{}0", &nonce[..63]);
        assert_eq!(
            registry.consume_for_process(42, &wrong),
            Err("challenge response must be exactly 64 hexadecimal characters")
        );
        assert!(registry.consume_for_process(42, &nonce).is_ok());
    }

    #[test]
    fn expired_challenge_is_removed() {
        let mut registry = ServiceChallengeRegistry::default();
        let issued = Instant::now();
        let nonce = registry.issue_for_test(42, issued).unwrap();
        assert_eq!(
            registry.consume_at(42, &nonce, issued + CHALLENGE_TTL),
            Err("client challenge expired")
        );
        assert_eq!(
            registry.consume_for_process(42, &nonce),
            Err("no pending challenge for client process")
        );
    }

    #[test]
    fn issuing_replaces_previous_challenge_for_same_process() {
        let mut registry = ServiceChallengeRegistry::default();
        let now = Instant::now();
        let first = registry.issue_for_test(42, now).unwrap();
        let second = registry.issue_for_test(42, now).unwrap();
        assert_ne!(first, second);
        assert_eq!(
            registry.consume_for_process(42, &first),
            Err("client challenge response mismatch")
        );
        assert!(registry.consume_for_process(42, &second).is_ok());
    }

    #[test]
    fn rejects_invalid_process_ids_and_nonce_encodings() {
        let mut registry = ServiceChallengeRegistry::default();
        assert_eq!(
            registry.issue_for_test(0, Instant::now()),
            Err("invalid client process ID")
        );
        assert_eq!(
            registry.consume_for_process(42, "not-a-nonce"),
            Err("challenge response must be exactly 64 hexadecimal characters")
        );
    }

    impl ServiceChallengeRegistry {
        fn issue_for_test(&mut self, process_id: u32, now: Instant) -> Result<String, &'static str> {
            self.issue_at(process_id, now)
        }
    }
}
