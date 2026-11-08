# Bridge Roadmap

## Foundation
- [x] Rust workspace and shared protocol crate
- [x] Windows, Android, and iOS native application shells
- [x] CI workflows for platform build validation
- [x] Device registry and pairing consent/expiry state machine

## Milestone 1: authenticated local connectivity — complete for Windows + Android

### Implemented foundations
- [x] Shared Rust mDNS/DNS-SD discovery and versioned device metadata
- [x] Windows DNS-SD advertisement and live unverified candidate UI
- [x] Android NSD publish/browse and candidate selection
- [x] iOS Bonjour publish/browse and candidate selection
- [x] Bounded length-prefixed TCP framing and socket timeouts
- [x] Noise XX initiator/responder handshake and encrypted transport
- [x] SHA-256 static-key fingerprints and pinned-fingerprint checks for previously trusted devices
- [x] Authenticated device descriptor exchange inside Noise transport
- [x] Windows Credential Manager, Android Keystore, and iOS Keychain identity persistence
- [x] Native Windows, Android, and iOS Noise pairing flows
- [x] Nearby-device Connect action and receiver-side one-tap approval on all three clients
- [x] Persistent trusted-device lists and revocation on all three clients
- [x] Existing trusted-device fingerprint pinning and changed-identity rejection
- [x] Replay rejection, wrong-fingerprint tests, and mobile FFI encrypted loopback integration tests
- [x] CI builds for Windows native app, Android APK, iOS Simulator app, and Rust workspace
- [x] Encrypted pairing-decision message: the receiver accepts or rejects, and the initiator waits for the authenticated response before saving trust
- [x] Mobile pending sessions are closed on rejection, persistence failure, app shutdown, and late completion after shutdown

### Validation outcome
- [x] Physical Windows ↔ Android discovery and pairing in both directions.
- [x] Receiver-side approval and trusted reconnection.
- [x] App restart/reconnection on Windows and Android.
- [x] Wi-Fi disconnect/reconnect; user reports working.
- [x] Windows firewall and Android network-permission behaviour; user reports working.
- [x] CI: Rust checks, native Windows build, Android APK, and iOS Simulator build.

### Deferred / follow-up validation
- [ ] Physical Windows ↔ iOS and Android ↔ iOS tests (requires iPhone and Mac).
- [ ] Linux desktop installation and interoperability tests (deferred until Linux is installed).
- [ ] Broader router/AP-isolation, IPv6-only, and lifecycle/resource-limit stress testing.
- [ ] Independent security review before broad public release.

**Milestone 1 is complete for the currently available Windows + Android target.** iOS and Linux physical-device/platform validation remain explicitly deferred; the completed milestone does not claim those platforms have passed real-device testing. See [docs/network-test.md](network-test.md) for the detailed record.

## Milestone 2: secure local file transfer — in progress

### Shared transfer engine
- [x] Dedicated Rust transfer crate shared by native clients
- [x] Bounded transfer-control messages and 48 KiB chunk framing for Noise transport
- [x] File-offer validation, 2 GiB initial size ceiling, and SHA-256 integrity verification
- [x] Safe filename validation and collision-safe, no-overwrite destination publication
- [x] Temporary-file cleanup for cancelled, interrupted, or invalid transfers
- [ ] Wire the transfer engine into authenticated desktop and Android sessions
- [ ] Incoming-file approval and destination selection UI on both platforms
- [ ] Streaming progress, cancellation, and completion/error states
- [ ] End-to-end Windows ↔ Android physical-device transfer tests
- [ ] Disconnect handling and interrupted-transfer cleanup tests
- [ ] Resume support (deferred until reliable non-resumable transfer is validated)

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
