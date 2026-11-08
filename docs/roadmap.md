# Bridge Roadmap

## Foundation
- [x] Rust workspace and shared protocol crate
- [x] Windows, Android, and iOS application shells
- [x] CI workflows for platform build validation
- [x] Device registry
- [x] Pairing consent and expiry state machine

## Milestone 1: authenticated local connectivity
- [x] Shared Rust local-network discovery module using mDNS/DNS-SD
- [x] Versioned discovery records with device IDs, platform, endpoint and public-key fingerprint
- [x] Bounded length-prefixed TCP framing
- [x] Noise XX initiator/responder transport primitives
- [x] SHA-256 public-key fingerprints and symmetric six-digit short authentication strings
- [x] Expected-fingerprint verification before a session is returned
- [ ] OS-backed persistent identity adapters (Windows DPAPI/Credential Manager, Android Keystore, Apple Keychain)
- [ ] Windows UI integration for live discovery and user-confirmed SAS
- [ ] Android NSD discovery + Keystore-backed Noise client integration
- [ ] iOS Bonjour discovery + Keychain-backed Noise client integration
- [ ] Persist trusted peers and revocation on each platform
- [ ] Cross-platform end-to-end tests on physical devices

## Device capabilities
- [ ] Capability permission model
- [ ] Universal clipboard (text first)
- [ ] Local file transfer with explicit destination approval
- [ ] Presence and device status
- [ ] Android notification bridge (subject to platform permissions)
- [ ] iOS-compatible feature set using public platform APIs

## Release quality
- [ ] Signed Windows installer
- [ ] Signed Android release bundle
- [ ] Apple signing and TestFlight distribution
- [ ] End-to-end tests across physical devices
