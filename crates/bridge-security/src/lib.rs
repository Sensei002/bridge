pub mod peer_attestation;
//! Noise XX secure sessions. Remote static keys must be verified and pinned
//! by the pairing layer before a session is trusted.
use snow::{HandshakeState, TransportState};
use thiserror::Error;

const NOISE_PATTERN: &str = "Noise_XX_25519_ChaChaPoly_SHA256";
const MAX_MESSAGE_SIZE: usize = 65_535;

#[derive(Debug, Error)]
pub enum SecurityError {
    #[error("Noise protocol error: {0}")]
    Noise(#[from] snow::Error),
    #[error("handshake is not complete")]
    HandshakeIncomplete,
    #[error("remote static key is not available")]
    MissingRemoteStaticKey,
}

#[derive(Clone, Debug)]
pub struct StaticKeypair {
    pub private: Vec<u8>,
    pub public: Vec<u8>,
}

/// Generate once per device and persist the private key in platform secure storage.
pub fn generate_static_keypair() -> Result<StaticKeypair, SecurityError> {
    let params = NOISE_PATTERN.parse()?;
    let pair = snow::Builder::new(params).generate_keypair()?;
    Ok(StaticKeypair { private: pair.private, public: pair.public })
}

/// Exchange messages in order: initiator -> responder, responder -> initiator,
/// initiator -> responder. The final remote static key must be user-verified.
pub struct NoiseHandshake {
    state: HandshakeState,
}

impl NoiseHandshake {
    pub fn initiator(private_key: &[u8]) -> Result<Self, SecurityError> {
        let params = NOISE_PATTERN.parse()?;
        let state = snow::Builder::new(params).local_private_key(private_key).build_initiator()?;
        Ok(Self { state })
    }

    pub fn responder(private_key: &[u8]) -> Result<Self, SecurityError> {
        let params = NOISE_PATTERN.parse()?;
        let state = snow::Builder::new(params).local_private_key(private_key).build_responder()?;
        Ok(Self { state })
    }

    pub fn write_message(&mut self, payload: &[u8]) -> Result<Vec<u8>, SecurityError> {
        let mut output = vec![0; MAX_MESSAGE_SIZE];
        let length = self.state.write_message(payload, &mut output)?;
        output.truncate(length);
        Ok(output)
    }

    pub fn read_message(&mut self, message: &[u8]) -> Result<Vec<u8>, SecurityError> {
        let mut payload = vec![0; MAX_MESSAGE_SIZE];
        let length = self.state.read_message(message, &mut payload)?;
        payload.truncate(length);
        Ok(payload)
    }

    pub fn is_finished(&self) -> bool {
        self.state.is_handshake_finished()
    }

    pub fn remote_static_key(&self) -> Result<&[u8], SecurityError> {
        self.state.get_remote_static().ok_or(SecurityError::MissingRemoteStaticKey)
    }

    pub fn into_transport(self) -> Result<EncryptedSession, SecurityError> {
        if !self.state.is_handshake_finished() {
            return Err(SecurityError::HandshakeIncomplete);
        }
        Ok(EncryptedSession { state: self.state.into_transport_mode()? })
    }
}

pub struct EncryptedSession {
    state: TransportState,
}

impl EncryptedSession {
    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, SecurityError> {
        let mut output = vec![0; plaintext.len() + 64];
        let length = self.state.write_message(plaintext, &mut output)?;
        output.truncate(length);
        Ok(output)
    }

    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, SecurityError> {
        let mut output = vec![0; ciphertext.len()];
        let length = self.state.read_message(ciphertext, &mut output)?;
        output.truncate(length);
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn establish() -> (EncryptedSession, EncryptedSession, Vec<u8>, Vec<u8>) {
        let initiator_keys = generate_static_keypair().unwrap();
        let responder_keys = generate_static_keypair().unwrap();
        let mut initiator = NoiseHandshake::initiator(&initiator_keys.private).unwrap();
        let mut responder = NoiseHandshake::responder(&responder_keys.private).unwrap();
        let first = initiator.write_message(b"").unwrap();
        responder.read_message(&first).unwrap();
        let second = responder.write_message(b"").unwrap();
        initiator.read_message(&second).unwrap();
        let third = initiator.write_message(b"").unwrap();
        responder.read_message(&third).unwrap();
        assert!(initiator.is_finished() && responder.is_finished());
        assert_eq!(initiator.remote_static_key().unwrap(), responder_keys.public);
        assert_eq!(responder.remote_static_key().unwrap(), initiator_keys.public);
        (
            initiator.into_transport().unwrap(),
            responder.into_transport().unwrap(),
            initiator_keys.public,
            responder_keys.public,
        )
    }

    #[test]
    fn noise_xx_establishes_encrypted_session() {
        let (mut sender, mut receiver, _, _) = establish();
        let encrypted = sender.encrypt(b"clipboard payload").unwrap();
        assert_ne!(encrypted, b"clipboard payload");
        assert_eq!(receiver.decrypt(&encrypted).unwrap(), b"clipboard payload");
    }

    #[test]
    fn transport_rejects_modified_ciphertext() {
        let (mut sender, mut receiver, _, _) = establish();
        let mut ciphertext = sender.encrypt(b"secret").unwrap();
        ciphertext[0] ^= 1;
        assert!(receiver.decrypt(&ciphertext).is_err());
    }
}
