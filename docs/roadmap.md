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
- [x] Initial OS-backed identity storage adapters (Windows Credential Manager, Android Keystore-wrapped storage, Apple Keychain)
- [x] Windows desktop mDNS/DNS-SD service advertisement and unverified candidate reporting
- [x] Android NSD and iOS Bonjour browsing for Bridge service candidates
- [x] Durable trust/revocation transaction APIs with rollback on storage failure
- [x] Replay rejection and pinned-fingerprint negative-path tests
- [ ] Windows UI integration for Noise handshake, matching SAS display, and user-confirmed trust
- [ ] Android NSD candidate selection + Keystore-backed Noise client integration
- [ ] iOS Bonjour candidate selection + Keychain-backed Noise client integration
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
