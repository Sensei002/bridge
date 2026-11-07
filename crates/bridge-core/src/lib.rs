//! Platform-independent Bridge domain logic.
use std::collections::HashMap;
use bridge_protocol::{DeviceDescriptor, DeviceId};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RegistryError {
    #[error("device is already registered")] AlreadyRegistered,
    #[error("device was not found")] NotFound,
}

/// In-memory registry; persistent identity and pairing are future milestones.
#[derive(Debug, Default)]
pub struct DeviceRegistry { devices: HashMap<DeviceId, DeviceDescriptor> }

impl DeviceRegistry {
    pub fn register(&mut self, device: DeviceDescriptor) -> Result<(), RegistryError> {
        if self.devices.contains_key(&device.id) { return Err(RegistryError::AlreadyRegistered); }
        self.devices.insert(device.id, device);
        Ok(())
    }
    pub fn get(&self, id: &DeviceId) -> Option<&DeviceDescriptor> { self.devices.get(id) }
    pub fn remove(&mut self, id: &DeviceId) -> Result<DeviceDescriptor, RegistryError> {
        self.devices.remove(id).ok_or(RegistryError::NotFound)
    }
    pub fn list(&self) -> Vec<&DeviceDescriptor> { self.devices.values().collect() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bridge_protocol::{Platform, PROTOCOL_VERSION};
    fn sample() -> DeviceDescriptor {
        DeviceDescriptor { id: DeviceId::new(), display_name: "Test device".into(),
            platform: Platform::Windows, protocol_version: PROTOCOL_VERSION }
    }
    #[test]
    fn registers_and_removes_device() {
        let mut registry = DeviceRegistry::default();
        let device = sample(); let id = device.id;
        registry.register(device).expect("register");
        assert!(registry.get(&id).is_some());
        registry.remove(&id).expect("remove");
        assert!(registry.get(&id).is_none());
    }
    #[test]
    fn rejects_duplicate_device() {
        let mut registry = DeviceRegistry::default();
        let device = sample();
        registry.register(device.clone()).expect("first register");
        assert_eq!(registry.register(device), Err(RegistryError::AlreadyRegistered));
    }
}
