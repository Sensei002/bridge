//! Domain-separated Android peer attestation primitives.
//!
//! The verifier MUST obtain the Ed25519 public key from the already-approved
//! pairing record. Never trust a public key supplied alongside an attestation.
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use thiserror::Error;
use uuid::Uuid;

const DOMAIN: &[u8] = b"BRIDGE_ANDROID_PEER_ATTESTATION_V1\0";
const FINGERPRINT_BYTES: usize = 64;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AttestationError {
    #[error("invalid signing key length")]
    InvalidKey,
    #[error("invalid service challenge length")]
    InvalidNonce,
    #[error("invalid peer fingerprint")]
    InvalidFingerprint,
    #[error("attestation does not match the expected challenge or session")]
    ContextMismatch,
    #[error("Android peer signature is invalid")]
    InvalidSignature,
}

/// A proof signed by the Android device's long-term pairing identity.
/// The service challenge and session ID are included in the signed transcript.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AndroidPeerAttestation {
    pub nonce: [u8; 32],
    pub session_id: Uuid,
    pub device_fingerprint: String,
    pub signature: [u8; 64],
}

/// Return the raw 32-byte Ed25519 public key for the pairing record.
pub fn attestation_public_key(seed: &[u8]) -> Result<[u8; 32], AttestationError> {
    let seed: [u8; 32] = seed.try_into().map_err(|_| AttestationError::InvalidKey)?;
    Ok(SigningKey::from_bytes(&seed).verifying_key().to_bytes())
}

/// Sign the service nonce using the Android identity private seed.
pub fn sign_android_peer_attestation(
    seed: &[u8],
    nonce: &[u8],
    session_id: Uuid,
    device_fingerprint: &str,
) -> Result<AndroidPeerAttestation, AttestationError> {
    let seed: [u8; 32] = seed.try_into().map_err(|_| AttestationError::InvalidKey)?;
    let nonce: [u8; 32] = nonce.try_into().map_err(|_| AttestationError::InvalidNonce)?;
    validate_fingerprint(device_fingerprint)?;
    let key = SigningKey::from_bytes(&seed);
    let signature = key.sign(&transcript(&nonce, session_id, device_fingerprint)).to_bytes();
    Ok(AndroidPeerAttestation {
        nonce,
        session_id,
        device_fingerprint: device_fingerprint.to_ascii_lowercase(),
        signature,
    })
}

/// Verify against the public key pinned during explicit device pairing.
pub fn verify_android_peer_attestation(
    pinned_public_key: &[u8],
    attestation: &AndroidPeerAttestation,
    expected_nonce: &[u8],
    expected_session_id: Uuid,
    expected_fingerprint: &str,
) -> Result<(), AttestationError> {
    let public_key: [u8; 32] = pinned_public_key.try_into().map_err(|_| AttestationError::InvalidKey)?;
    let expected_nonce: [u8; 32] = expected_nonce.try_into().map_err(|_| AttestationError::InvalidNonce)?;
    validate_fingerprint(expected_fingerprint)?;
    if attestation.nonce != expected_nonce
        || attestation.session_id != expected_session_id
        || !attestation.device_fingerprint.eq_ignore_ascii_case(expected_fingerprint)
    {
        return Err(AttestationError::ContextMismatch);
    }
    let key = VerifyingKey::from_bytes(&public_key).map_err(|_| AttestationError::InvalidKey)?;
    let signature = Signature::from_bytes(&attestation.signature);
    key.verify_strict(
        &transcript(&attestation.nonce, attestation.session_id, &attestation.device_fingerprint),
        &signature,
    ).map_err(|_| AttestationError::InvalidSignature)
}

fn validate_fingerprint(value: &str) -> Result<(), AttestationError> {
    if value.len() != FINGERPRINT_BYTES || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AttestationError::InvalidFingerprint);
    }
    Ok(())
}

fn transcript(nonce: &[u8; 32], session_id: Uuid, fingerprint: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(DOMAIN.len() + 32 + 16 + FINGERPRINT_BYTES);
    bytes.extend_from_slice(DOMAIN);
    bytes.extend_from_slice(nonce);
    bytes.extend_from_slice(session_id.as_bytes());
    bytes.extend_from_slice(fingerprint.to_ascii_lowercase().as_bytes());
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    const FINGERPRINT: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn proof() -> ([u8; 32], [u8; 32], Uuid, AndroidPeerAttestation) {
        let seed = [7_u8; 32];
        let public = attestation_public_key(&seed).unwrap();
        let nonce = [9_u8; 32];
        let session = Uuid::new_v4();
        let attestation = sign_android_peer_attestation(&seed, &nonce, session, FINGERPRINT).unwrap();
        (public, nonce, session, attestation)
    }

    #[test]
    fn paired_android_key_verifies_service_nonce_and_session() {
        let (public, nonce, session, attestation) = proof();
        assert_eq!(verify_android_peer_attestation(&public, &attestation, &nonce, session, FINGERPRINT), Ok(()));
    }

    #[test]
    fn another_devices_key_cannot_forge_a_pinned_peers_proof() {
        let (_, nonce, session, attestation) = proof();
        let other = attestation_public_key(&[8_u8; 32]).unwrap();
        assert_eq!(verify_android_peer_attestation(&other, &attestation, &nonce, session, FINGERPRINT), Err(AttestationError::InvalidSignature));
    }

    #[test]
    fn rejects_replayed_or_cross_session_challenges() {
        let (public, nonce, session, attestation) = proof();
        assert_eq!(verify_android_peer_attestation(&public, &attestation, &[1_u8; 32], session, FINGERPRINT), Err(AttestationError::ContextMismatch));
        assert_eq!(verify_android_peer_attestation(&public, &attestation, &nonce, Uuid::new_v4(), FINGERPRINT), Err(AttestationError::ContextMismatch));
    }

    #[test]
    fn rejects_wrong_peer_fingerprint_and_malformed_inputs() {
        let (public, nonce, session, attestation) = proof();
        assert_eq!(verify_android_peer_attestation(&public, &attestation, &nonce, session, &"f".repeat(64)), Err(AttestationError::ContextMismatch));
        assert_eq!(sign_android_peer_attestation(&[1; 31], &nonce, session, FINGERPRINT), Err(AttestationError::InvalidKey));
        assert_eq!(sign_android_peer_attestation(&[1; 32], &[0; 31], session, FINGERPRINT), Err(AttestationError::InvalidNonce));
        assert_eq!(sign_android_peer_attestation(&[1; 32], &nonce, session, "bad"), Err(AttestationError::InvalidFingerprint));
    }

    #[test]
    fn modified_signature_is_rejected() {
        let (public, nonce, session, mut attestation) = proof();
        attestation.signature[0] ^= 1;
        assert_eq!(verify_android_peer_attestation(&public, &attestation, &nonce, session, FINGERPRINT), Err(AttestationError::InvalidSignature));
    }
}
