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


## Milestone 2: Android as a native extended Windows display — active

**Goal:** a trusted Android phone or tablet appears as a real monitor in Windows Settings → System → Display. The user can choose **Extend**, position the display, and move normal application windows onto it. A mirrored desktop preview alone does not satisfy this milestone.

### Implemented in the development code
- [x] Select Microsoft IddCx / Indirect Display Driver (UMDF) as the Windows virtual-monitor architecture.
- [x] Add the Windows IddCx driver project and separate Bridge Display Service.
- [x] Add restricted driver connect/disconnect/status controls and service-side authenticated control flow.
- [x] Build and package the Windows driver and display service in GitHub Actions.
- [x] Implement Windows Media Foundation H.264 encoding and bounded access-unit transmission over the authenticated Bridge session.
- [x] Implement Android H.264 access-unit reception and MediaCodec Surface rendering with SPS/PPS configuration.
- [x] Add dynamic display-size handling, keyframe recovery requests, throttling, and BGRA fallback.
- [x] Add Windows and Android diagnostic logging with Settings → Show Logs / Copy Logs.

### Remaining acceptance gates
- [ ] Validate signed driver distribution, installation, service registration, virtual-monitor arrival/removal, and safe mode negotiation on a physical Windows host.
- [ ] Validate actual IddCx Direct3D swap-chain frame acquisition and delivery into the media pipeline.
- [ ] Confirm sustained H.264 playback and IDR/SPS/PPS recovery on the target Windows PC and Redmi Note 7 Pro.
- [ ] Validate rotation, orientation changes, sleep/wake, Wi-Fi recovery, and decoder recreation on hardware.
- [ ] Complete Windows Extend-mode acceptance: move real application windows onto the Android display and see them rendered there.
- [ ] Implement end-to-end touch/pen input forwarding from Android to Windows.
- [ ] Measure end-to-end latency, frame pacing, bitrate, CPU/GPU use, and thermal impact; add adaptive bitrate/congestion handling.
- [ ] Validate installer upgrade, rollback, removal, and recovery behaviour.

**Current status:** software components and CI build paths are in place, but physical acceptance is not yet complete. CI success or a published development release does not prove the driver installs, the virtual monitor appears, or live video works on the target devices.

## Milestone 3: secure local file transfer — planned

The shared Rust transfer engine has foundational bounded framing, offer validation, safe filename handling, and SHA-256 integrity verification. Authenticated-session integration, incoming-file approval UI, progress/cancellation, and physical-device transfer tests remain future work. File transfer is not the current active milestone.

## Other device capabilities
- [ ] Capability permission model
- [ ] Universal clipboard (text first)
- [ ] Local file transfer with explicit destination approval (planned under Milestone 3)
- [ ] Presence and device status
- [ ] Android notification bridge (subject to platform permissions)
- [ ] iOS-compatible feature set using public platform APIs

## Release quality
- [ ] Signed Windows installer
- [ ] Signed Android release bundle
- [ ] Apple signing and TestFlight distribution
- [ ] End-to-end tests across physical devices
