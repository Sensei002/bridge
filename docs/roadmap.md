# Bridge Roadmap

## Foundation
- [x] Rust workspace and shared protocol crate
- [x] Windows, Android, and iOS native application shells
- [x] CI workflows for platform build validation
- [x] Device registry and pairing consent/expiry state machine

## Milestone 1: authenticated local connectivity

### Implemented foundations
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
- [x] Secure QR invitation data model, strict validation, expiry, bounded one-time nonce replay guard, and URI codec
- [x] Rust FFI/JNI/Swift entry points for QR invitation creation and consumption
- [x] Scanned invitation endpoint, expected Noise fingerprint, and expected device ID are passed into the mobile pairing controllers; the authenticated descriptor ID is checked before session registration
- [x] Mobile pending sessions are closed on rejection, persistence failure, app shutdown, and late completion after shutdown

### Remaining completion gates
- [x] Native QR code rendering and invitation display in Windows egui, Android Compose, and iOS SwiftUI pairing UI
- [x] Native camera scanning on Android (Google Code Scanner) and iOS (VisionKit), wired to the shared Rust one-time invitation consumer
- [ ] Complete scanned-invitation pairing path physically verified from UI to Noise XX handshake on every platform
- [ ] Physical Windows ↔ Android end-to-end pairing and reconnection tests (see docs/network-test.md)
- [ ] Physical Windows ↔ iOS end-to-end pairing and reconnection tests (see docs/network-test.md)
- [ ] Physical Android ↔ iOS end-to-end pairing and reconnection tests (see docs/network-test.md)
- [ ] Validate on different Wi-Fi routers, client isolation settings, IPv4/IPv6 environments, sleep/resume, and network changes (see docs/network-test.md)
- [ ] Connection lifecycle/resource-limit hardening, graceful shutdown, and platform permission/error handling
- [ ] Independent security review of discovery, QR bootstrap, Noise identity pinning, trust persistence, and revocation

**Milestone 1 is not complete until every remaining completion gate above is closed.** CI and simulator/loopback tests are necessary but do not replace physical-device interoperability tests or an independent security review.

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
