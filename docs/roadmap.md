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

## Milestone 2: Android as a native extended Windows display

Architecture and acceptance gates: [docs/second-screen-architecture.md](second-screen-architecture.md).

### Windows virtual display foundation
- [x] Stop developing/signing a Bridge-owned IddCx driver and select upstream VirtualDrivers VDD 25.7.23.
- [x] Verify `SignedDrivers/x86/VDD/MttVDD.dll` is AMD64 (`0x8664`) despite the folder name.
- [x] Verify the catalog signature is valid and signed by SignPath Foundation; pin the smaller upstream driver-only archive `VirtualDisplayDriver-x86.Driver.Only.zip` at SHA-256 `e24210692b442b39af763536330ce78b423f19342b7a7792c26de3944e418b3a`.
- [x] Verify the INF and DLL with Windows PnP package-signing policy (`signtool verify /pa`); this is an UMDF user-mode driver, so kernel-mode `/kp` is not applicable.
- [ ] Confirm redistribution terms for the exact driver-only files before bundling them; do not include the VDD control companion app unless its terms are confirmed.
- [ ] Replace Bridge-specific driver IOCTL control with a narrow client for the upstream `\\.\\pipe\\MTTVirtualDisplayPipe` control interface.
- [ ] Identify the VDD virtual output reliably through Windows display topology APIs.
- [ ] Prototype DXGI Desktop Duplication capture of the VDD virtual output; evaluate Windows Graphics Capture if needed.
- [ ] Route captured frames into the existing Bridge Media Foundation H.264 and authenticated Android pipeline.
- [ ] Implement elevated, rollback-capable installation/uninstallation of the vendor-signed VDD package; no Bridge driver signing or test-signing.
- [ ] Validate Windows Extend mode on a physical Windows machine: move ordinary application windows onto the Android display.

### Real-time desktop media pipeline
- [ ] Capture the selected VDD virtual output through DXGI Desktop Duplication or Windows Graphics Capture without blocking display composition.
- [ ] Add GPU-assisted H.264 hardware encoding and bounded frame queues.
- [ ] Add a dedicated low-latency media transport authenticated to the trusted Bridge device identity.
- [ ] Add adaptive bitrate, frame pacing, timestamps, and packet-loss recovery.
- [ ] Keep media traffic independent from file transfers and control messages.

### Android second-screen renderer
- [ ] Add a dedicated full-screen Android Second Screen mode using MediaCodec hardware decoding and SurfaceView.
- [ ] Negotiate resolution, orientation, and refresh rate with the Windows virtual monitor.
- [ ] Support connect/disconnect, Wi-Fi recovery, screen sleep/wake, and decoder recreation.
- [ ] Validate Windows Extend mode with real application windows on the Android display.

### Reliability and performance validation
- [ ] Measure end-to-end latency, frame pacing, bitrate, CPU/GPU use, and thermal impact.
- [ ] Test display hotplug, Windows sleep/resume, lock/unlock, GPU reset, and service/driver recovery.
- [ ] Test network loss/roaming and Android rotation/process lifecycle.
- [ ] Verify safe uninstall and that the physical display remains usable after driver/service failure.

\n## Milestone 3: secure local file transfer — in progress

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
- [x] Explicit text sharing between connected trusted Windows and Android devices (16 KiB cap; opt-in; no background clipboard monitoring)
- [ ] Automatic clipboard monitoring/sync (deferred for Android background-access restrictions and additional privacy controls)
- [ ] Local file transfer with explicit destination approval
- [ ] Presence and device status
- [ ] Android notification bridge (subject to platform permissions)
- [ ] iOS-compatible feature set using public platform APIs

## Release quality
- [ ] Signed Windows installer
- [ ] Signed Android release bundle
- [ ] Apple signing and TestFlight distribution
- [ ] End-to-end tests across physical devices
