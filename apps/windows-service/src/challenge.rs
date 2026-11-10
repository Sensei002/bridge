//! One-time, process-bound challenges for the Windows display-service handshake.
//!
//! The named-pipe server exposes this registry only after verifying the desktop
//! executable's publisher and retaining the exact live process handle.
//! A PID alone is not a durable process identity and must never authorize a lease.

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

/// Challenges must be answered quickly and may be consumed exactly once.
pub const CHALLENGE_TTL: Duration = Duration::from_secs(10);
const NONCE_BYTES: usize = 32;

/// A pending challenge is bound to the process that requested it.
#[derive(Debug, Clone)]
struct PendingChallenge {
    nonce: [u8; NONCE_BYTES],
    expires_at: Instant,
    attestation: Option<PendingAttestation>,
}

#[derive(Debug, Clone)]
struct PendingAttestation {
    session_id: uuid::Uuid,
    device_fingerprint: String,
    pinned_public_key: [u8; 32],
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


    /// Create a challenge that can only be consumed by a signed proof from the
    /// Android identity and live session already selected by the trusted desktop.
    /// The service retains the pairing-pinned public key; it is never accepted
    /// from the proof response itself.
    pub fn issue_attestation_after_client_verification(
        &mut self,
        process_id: u32,
        session_id: uuid::Uuid,
        device_fingerprint: &str,
        pinned_public_key: &[u8],
    ) -> Result<String, &'static str> {
        if process_id == 0 {
            return Err("invalid client process ID");
        }
        if device_fingerprint.len() != 64
            || !device_fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("invalid Android device fingerprint");
        }
        let pinned_public_key: [u8; 32] = pinned_public_key
            .try_into()
            .map_err(|_| "pinned Android attestation key must be 32 bytes")?;
        let mut nonce = [0_u8; NONCE_BYTES];
        getrandom::fill(&mut nonce)
            .map_err(|_| "cryptographic random generator unavailable")?;
        self.pending.insert(process_id, PendingChallenge {
            nonce,
            expires_at: Instant::now() + CHALLENGE_TTL,
            attestation: Some(PendingAttestation {
                session_id,
                device_fingerprint: device_fingerprint.to_ascii_lowercase(),
                pinned_public_key,
            }),
        });
        Ok(encode_hex(&nonce))
    }

    /// Consume and independently verify a signed Android proof. A well-formed
    /// attempt consumes the challenge even when verification fails, preventing
    /// repeated guesses against the same nonce.
    pub fn consume_signed_attestation_for_process(
        &mut self,
        process_id: u32,
        nonce_hex: &str,
        session_id: uuid::Uuid,
        device_fingerprint: &str,
        signature_hex: &str,
    ) -> Result<(), &'static str> {
        if process_id == 0 {
            return Err("invalid client process ID");
        }
        let nonce = decode_hex(nonce_hex)?;
        let signature = decode_fixed_hex::<64>(signature_hex, "attestation signature must be 128 hexadecimal characters")?;
        let pending = self.pending.remove(&process_id)
            .ok_or("no pending challenge for client process")?;
        if Instant::now() >= pending.expires_at {
            return Err("client challenge expired");
        }
        let context = pending.attestation.ok_or("pending challenge does not require signed attestation")?;
        let proof = bridge_security::peer_attestation::AndroidPeerAttestation {
            nonce: pending.nonce,
            session_id,
            device_fingerprint: device_fingerprint.to_owned(),
            signature,
        };
        bridge_security::peer_attestation::verify_android_peer_attestation(
            &context.pinned_public_key,
            &proof,
            &pending.nonce,
            context.session_id,
            &context.device_fingerprint,
        ).map_err(|_| "Android attestation signature or context verification failed")?;
        if nonce.as_slice() != pending.nonce {
            return Err("Android attestation nonce does not match service challenge");
        }
        Ok(())
    }

    /// Remove expired challenges and return their process IDs so the caller can
    /// release the corresponding retained process handles.
    pub fn revoke_expired(&mut self) -> Vec<u32> {
        self.revoke_expired_at(Instant::now())
    }

    fn revoke_expired_at(&mut self, now: Instant) -> Vec<u32> {
        let expired = self.pending.iter()
            .filter_map(|(process_id, pending)| (now >= pending.expires_at).then_some(*process_id))
            .collect::<Vec<_>>();
        for process_id in &expired {
            self.pending.remove(process_id);
        }
        expired
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

        let mut nonce = [0_u8; NONCE_BYTES];
        getrandom::fill(&mut nonce).map_err(|_| "cryptographic random generator unavailable")?;
        self.pending.insert(
            process_id,
            PendingChallenge {
                nonce,
                expires_at: now + CHALLENGE_TTL,
                attestation: None,
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
        if pending.attestation.is_some() {
            return Err("signed Android attestation is required for this challenge");
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


fn decode_fixed_hex<const N: usize>(value: &str, error: &'static str) -> Result<[u8; N], &'static str> {
    if value.len() != N * 2 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(error);
    }
    let mut output = [0_u8; N];
    for (index, byte) in output.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| error)?;
    }
    Ok(output)
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
        let replacement = if nonce.as_bytes()[63] == b'0' { '1' } else { '0' };
        let wrong = format!("{}{}", &nonce[..63], replacement);
        assert_eq!(
            registry.consume_for_process(42, &wrong),
            Err("client challenge response mismatch")
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
    fn expiry_cleanup_returns_process_ids_for_handle_release() {
        let mut registry = ServiceChallengeRegistry::default();
        let issued = Instant::now();
        registry.issue_at(42, issued).unwrap();
        registry.issue_at(43, issued).unwrap();
        let mut expired = registry.revoke_expired_at(issued + CHALLENGE_TTL);
        expired.sort_unstable();
        assert_eq!(expired, vec![42, 43]);
        assert!(registry.pending.is_empty());
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


    #[test]
    fn signed_attestation_is_verified_by_service_against_pinned_pairing_key() {
        const FINGERPRINT: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let seed = [7_u8; 32];
        let public_key = bridge_security::peer_attestation::attestation_public_key(&seed).unwrap();
        let session_id = uuid::Uuid::new_v4();
        let mut registry = ServiceChallengeRegistry::default();
        let nonce_hex = registry.issue_attestation_after_client_verification(
            42, session_id, FINGERPRINT, &public_key
        ).unwrap();
        assert_eq!(
            registry.consume_for_process(42, &nonce_hex),
            Err("signed Android attestation is required for this challenge")
        );
        let nonce = decode_hex(&nonce_hex).unwrap();
        let proof = bridge_security::peer_attestation::sign_android_peer_attestation(
            &seed, &nonce, session_id, FINGERPRINT
        ).unwrap();
        assert_eq!(
            registry.consume_signed_attestation_for_process(
                42, &nonce_hex, proof.session_id, &proof.device_fingerprint, &encode_hex(&proof.signature)
            ),
            Ok(())
        );
        assert_eq!(
            registry.consume_signed_attestation_for_process(
                42, &nonce_hex, proof.session_id, &proof.device_fingerprint, &encode_hex(&proof.signature)
            ),
            Err("no pending challenge for client process")
        );
    }

    #[test]
    fn signed_attestation_rejects_wrong_key_and_consumes_failed_attempt() {
        const FINGERPRINT: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let trusted_seed = [7_u8; 32];
        let attacker_seed = [8_u8; 32];
        let public_key = bridge_security::peer_attestation::attestation_public_key(&trusted_seed).unwrap();
        let session_id = uuid::Uuid::new_v4();
        let mut registry = ServiceChallengeRegistry::default();
        let nonce_hex = registry.issue_attestation_after_client_verification(
            42, session_id, FINGERPRINT, &public_key
        ).unwrap();
        let nonce = decode_hex(&nonce_hex).unwrap();
        let forged = bridge_security::peer_attestation::sign_android_peer_attestation(
            &attacker_seed, &nonce, session_id, FINGERPRINT
        ).unwrap();
        assert_eq!(
            registry.consume_signed_attestation_for_process(
                42, &nonce_hex, session_id, FINGERPRINT, &encode_hex(&forged.signature)
            ),
            Err("Android attestation signature or context verification failed")
        );
        assert_eq!(
            registry.consume_signed_attestation_for_process(
                42, &nonce_hex, session_id, FINGERPRINT, &encode_hex(&forged.signature)
            ),
            Err("no pending challenge for client process")
        );
    }

    #[test]
    fn signed_attestation_rejects_session_or_fingerprint_mismatch() {
        const FINGERPRINT: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let seed = [7_u8; 32];
        let public_key = bridge_security::peer_attestation::attestation_public_key(&seed).unwrap();
        let session_id = uuid::Uuid::new_v4();
        let mut registry = ServiceChallengeRegistry::default();
        let nonce_hex = registry.issue_attestation_after_client_verification(
            42, session_id, FINGERPRINT, &public_key
        ).unwrap();
        let nonce = decode_hex(&nonce_hex).unwrap();
        let proof = bridge_security::peer_attestation::sign_android_peer_attestation(
            &seed, &nonce, session_id, FINGERPRINT
        ).unwrap();
        assert_eq!(
            registry.consume_signed_attestation_for_process(
                42, &nonce_hex, uuid::Uuid::new_v4(), FINGERPRINT, &encode_hex(&proof.signature)
            ),
            Err("Android attestation signature or context verification failed")
        );
    }

    impl ServiceChallengeRegistry {
        fn issue_for_test(&mut self, process_id: u32, now: Instant) -> Result<String, &'static str> {
            self.issue_at(process_id, now)
        }
    }
}
