//! Persistent public trust records for devices that completed user-verified pairing.
//!
//! This store intentionally contains public identity metadata only. Private
//! device keys belong in each platform's secure storage (DPAPI/Keychain/Keystore).
use std::{collections::HashMap, fs, io, path::Path};

use bridge_protocol::DeviceDescriptor;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const STORE_VERSION: u16 = 1;

#[derive(Debug, Error)]
pub enum TrustStoreError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("trust store serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("unsupported trust store version: {0}")]
    UnsupportedVersion(u16),
    #[error("device descriptor is invalid: {0}")]
    InvalidDevice(String),
    #[error("fingerprint must be 64 hexadecimal characters")]
    InvalidFingerprint,
    #[error("the displayed authentication codes did not match")]
    AuthenticationCodeMismatch,
    #[error("device is already trusted")]
    AlreadyTrusted,
    #[error("device is not trusted")]
    NotTrusted,
    #[error("duplicate device ID in trust store")]
    DuplicateDeviceId,
    #[error("peer identity does not match the pinned fingerprint")]
    IdentityMismatch,
}

/// Represents an explicit first-use trust decision for an authenticated peer.
#[derive(Clone, Debug)]
pub struct SasConfirmation {
    fingerprint: String,
}

impl SasConfirmation {
    pub fn user_confirmed(
        handshake_code: &str,
        local_code: &str,
        remote_code: &str,
        remote_fingerprint: &str,
    ) -> Result<Self, TrustStoreError> {
        let valid_code = |code: &str| {
            code.len() == 6 && code.bytes().all(|byte| byte.is_ascii_digit())
        };
        if !valid_code(handshake_code)
            || !valid_code(local_code)
            || !valid_code(remote_code)
            || handshake_code != local_code
            || handshake_code != remote_code
        {
            return Err(TrustStoreError::AuthenticationCodeMismatch);
        }
        validate_fingerprint(remote_fingerprint)?;
        Ok(Self { fingerprint: remote_fingerprint.to_ascii_lowercase() })
    }

    /// Create trust after the receiving device explicitly approves the authenticated
    /// Noise peer. This is trust-on-first-use: the remote identity is cryptographically
    /// bound to this session, while the user confirms the device name/platform in UI.
    pub fn user_approved(remote_fingerprint: &str) -> Result<Self, TrustStoreError> {
        validate_fingerprint(remote_fingerprint)?;
        Ok(Self { fingerprint: remote_fingerprint.to_ascii_lowercase() })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrustedDevice {
    pub descriptor: DeviceDescriptor,
    /// SHA-256 fingerprint of the peer's Noise static public key.
    pub public_key_fingerprint: String,
    pub paired_at_ms: u64,
    pub last_seen_ms: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct TrustStoreFile {
    version: u16,
    devices: Vec<TrustedDevice>,
}

/// Persistent registry of explicitly approved peer identities.
/// The file contains public keys' fingerprints, never private key material.
#[derive(Debug, Default)]
pub struct TrustStore {
    devices: HashMap<bridge_protocol::DeviceId, TrustedDevice>,
}

impl TrustStore {
    pub fn new() -> Self { Self::default() }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, TrustStoreError> {
        let path = path.as_ref();
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::new()),
            Err(error) => return Err(error.into()),
        };
        let file: TrustStoreFile = serde_json::from_slice(&bytes)?;
        if file.version != STORE_VERSION {
            return Err(TrustStoreError::UnsupportedVersion(file.version));
        }
        let mut store = Self::new();
        for device in file.devices {
            device.descriptor.validate()
                .map_err(|error| TrustStoreError::InvalidDevice(error.to_string()))?;
            validate_fingerprint(&device.public_key_fingerprint)?;
            if store.devices.insert(device.descriptor.id, device).is_some() {
                return Err(TrustStoreError::DuplicateDeviceId);
            }
        }
        Ok(store)
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), TrustStoreError> {
        let path = path.as_ref();
        let parent = path.parent().filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let file = TrustStoreFile {
            version: STORE_VERSION,
            devices: self.devices.values().cloned().collect(),
        };
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        serde_json::to_writer_pretty(&mut temporary, &file)?;
        use std::io::Write;
        temporary.flush()?;
        temporary.as_file().sync_all()?;
        temporary.persist(path).map_err(|error| TrustStoreError::Io(error.error))?;
        Ok(())
    }

    pub fn trust(
        &mut self,
        descriptor: DeviceDescriptor,
        confirmation: SasConfirmation,
        now_ms: u64,
    ) -> Result<(), TrustStoreError> {
        descriptor.validate()
            .map_err(|error| TrustStoreError::InvalidDevice(error.to_string()))?;
        if self.devices.contains_key(&descriptor.id) {
            return Err(TrustStoreError::AlreadyTrusted);
        }
        let trusted = TrustedDevice {
            descriptor: descriptor.clone(),
            public_key_fingerprint: confirmation.fingerprint,
            paired_at_ms: now_ms,
            last_seen_ms: now_ms,
        };
        self.devices.insert(descriptor.id, trusted);
        Ok(())
    }

    /// Trust a device and persist the updated registry as one operation.
    /// In-memory state is rolled back if the durable write fails.
    pub fn trust_and_save(
        &mut self,
        descriptor: DeviceDescriptor,
        confirmation: SasConfirmation,
        now_ms: u64,
        path: impl AsRef<Path>,
    ) -> Result<(), TrustStoreError> {
        let previous = self.devices.clone();
        self.trust(descriptor, confirmation, now_ms)?;
        if let Err(error) = self.save(path) {
            self.devices = previous;
            return Err(error);
        }
        Ok(())
    }

    pub fn revoke(&mut self, id: &bridge_protocol::DeviceId) -> Result<TrustedDevice, TrustStoreError> {
        self.devices.remove(id).ok_or(TrustStoreError::NotTrusted)
    }

    /// Revoke a device and persist the change; restore it in memory on failure.
    pub fn revoke_and_save(
        &mut self,
        id: &bridge_protocol::DeviceId,
        path: impl AsRef<Path>,
    ) -> Result<TrustedDevice, TrustStoreError> {
        let previous = self.devices.clone();
        let removed = self.revoke(id)?;
        if let Err(error) = self.save(path) {
            self.devices = previous;
            return Err(error);
        }
        Ok(removed)
    }

    pub fn get(&self, id: &bridge_protocol::DeviceId) -> Option<&TrustedDevice> {
        self.devices.get(id)
    }

    pub fn list(&self) -> Vec<&TrustedDevice> {
        self.devices.values().collect()
    }

    pub fn is_trusted(&self, id: &bridge_protocol::DeviceId) -> bool {
        self.devices.contains_key(id)
    }

    pub fn verify_fingerprint(
        &self,
        id: &bridge_protocol::DeviceId,
        observed_fingerprint: &str,
    ) -> Result<(), TrustStoreError> {
        let trusted = self.devices.get(id).ok_or(TrustStoreError::NotTrusted)?;
        validate_fingerprint(observed_fingerprint)?;
        if trusted.public_key_fingerprint.eq_ignore_ascii_case(observed_fingerprint) {
            Ok(())
        } else {
            Err(TrustStoreError::IdentityMismatch)
        }
    }

    pub fn update_last_seen(
        &mut self,
        id: &bridge_protocol::DeviceId,
        now_ms: u64,
    ) -> Result<(), TrustStoreError> {
        let trusted = self.devices.get_mut(id).ok_or(TrustStoreError::NotTrusted)?;
        trusted.last_seen_ms = now_ms.max(trusted.last_seen_ms);
        Ok(())
    }
}

fn validate_fingerprint(value: &str) -> Result<(), TrustStoreError> {
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(TrustStoreError::InvalidFingerprint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use bridge_protocol::{DeviceId, Platform, PROTOCOL_VERSION};

    fn device() -> DeviceDescriptor {
        DeviceDescriptor {
            id: DeviceId::new(),
            display_name: "Bridge phone".into(),
            platform: Platform::Android,
            protocol_version: PROTOCOL_VERSION,
        }
    }

    fn device_with_id(id: DeviceId) -> DeviceDescriptor {
        DeviceDescriptor {
            id,
            display_name: "Bridge phone".into(),
            platform: Platform::Android,
            protocol_version: PROTOCOL_VERSION,
        }
    }

    fn confirmation() -> SasConfirmation {
        SasConfirmation::user_confirmed("042681", "042681", "042681", &"ab".repeat(32)).unwrap()
    }

    #[test]
    fn trust_requires_matching_user_confirmed_sas() {
        assert!(matches!(
            SasConfirmation::user_confirmed("123456", "123456", "123457", &"ab".repeat(32)),
            Err(TrustStoreError::AuthenticationCodeMismatch)
        ));
        assert!(matches!(
            SasConfirmation::user_confirmed("654321", "123456", "123456", &"ab".repeat(32)),
            Err(TrustStoreError::AuthenticationCodeMismatch)
        ));
        assert!(SasConfirmation::user_confirmed("123456", "123456", "123456", &"ab".repeat(32)).is_ok());
    }

    #[test]
    fn persists_and_revokes_trusted_devices() {
        let directory = tempfile::tempdir().unwrap();
        let path: PathBuf = directory.path().join("trusted-devices.json");
        let mut store = TrustStore::new();
        let descriptor = device();
        let id = descriptor.id;
        store.trust(descriptor, confirmation(), 100).unwrap();
        store.save(&path).unwrap();

        let mut loaded = TrustStore::load(&path).unwrap();
        assert!(loaded.is_trusted(&id));
        loaded.verify_fingerprint(&id, &"AB".repeat(32)).unwrap();
        assert!(loaded.verify_fingerprint(&id, &"cd".repeat(32)).is_err());
        loaded.update_last_seen(&id, 90).unwrap();
        assert_eq!(loaded.get(&id).unwrap().last_seen_ms, 100);
        loaded.revoke(&id).unwrap();
        assert!(!loaded.is_trusted(&id));
        loaded.save(&path).unwrap();
        assert!(TrustStore::load(&path).unwrap().list().is_empty());
    }

    #[test]
    fn trust_and_revoke_are_durable_operations() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("trusted-devices.json");
        let mut store = TrustStore::new();
        let descriptor = device();
        let id = descriptor.id;
        store.trust_and_save(descriptor, confirmation(), 500, &path).unwrap();
        assert!(TrustStore::load(&path).unwrap().is_trusted(&id));
        store.revoke_and_save(&id, &path).unwrap();
        assert!(!TrustStore::load(&path).unwrap().is_trusted(&id));
    }

    #[test]
    fn failed_durable_trust_and_revoke_roll_back_memory() {
        let directory = tempfile::tempdir().unwrap();
        let invalid_target = directory.path();
        let descriptor = device();
        let id = descriptor.id;
        let mut store = TrustStore::new();

        assert!(store.trust_and_save(descriptor, confirmation(), 100, invalid_target).is_err());
        assert!(!store.is_trusted(&id));

        let valid_path = directory.path().join("trusted-devices.json");
        store.trust_and_save(device_with_id(id), confirmation(), 100, &valid_path).unwrap();
        assert!(store.revoke_and_save(&id, invalid_target).is_err());
        assert!(store.is_trusted(&id));
    }

    #[test]
    fn missing_store_starts_empty() {
        let directory = tempfile::tempdir().unwrap();
        let store = TrustStore::load(directory.path().join("missing.json")).unwrap();
        assert!(store.list().is_empty());
    }
}
