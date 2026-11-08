# Bridge Roadmap

## Foundation
- [x] Rust workspace and shared protocol crate
- [x] Windows, Android, and iOS native application shells
- [x] CI workflows for platform build validation
- [x] Device registry and pairing consent/expiry state machine

## Milestone 1: authenticated local connectivity
- [x] Shared Rust mDNS/DNS-SD discovery and versioned device metadata
- [x] Windows DNS-SD advertisement and live unverified candidate UI
- [x] Android NSD publish/browse and candidate selection
- [x] iOS Bonjour publish/browse and candidate selection
- [x] Bounded length-prefixed TCP framing and socket timeouts
- [x] Noise XX initiator/responder handshake and encrypted transport
- [x] SHA-256 static-key fingerprints, symmetric six-digit SAS, and pinned-fingerprint checks
- [x] Authenticated device descriptor exchange inside Noise transport
- [x] Windows Credential Manager, Android Keystore, and iOS Keychain identity persistence
- [x] Native Windows, Android, and iOS Noise pairing flows
- [x] Matching SAS comparison and explicit first-time trust approval on all three clients
- [x] Persistent trusted-device lists and revocation on all three clients
- [x] Existing trusted-device fingerprint pinning and changed-identity rejection
- [x] Replay rejection, wrong-fingerprint tests, and mobile FFI encrypted loopback integration tests
- [x] CI builds for Windows native app, Android APK, iOS Simulator app, and Rust workspace
- [ ] Secure QR invitation/bootstrap flow (old unauthenticated prototype remains disabled)
- [ ] Physical-device end-to-end testing across Windows, Android, and iOS
- [ ] Network lifecycle/connection-limit hardening and independent security review

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
