# Bridge Roadmap

The active product milestone is **Milestone 2: Android as a real extended Windows display (Second Screen)**. Milestone 1 is complete for the physically validated Windows ↔ Android pairing target. File transfer is retained as a later capability and is not the current milestone.

## Foundation — complete
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

Milestone 1 is complete for the currently available Windows + Android target. iOS and Linux physical-device/platform validation remain explicitly deferred; completion does not claim those platforms have passed real-device testing. See [docs/network-test.md](network-test.md) for the detailed record.

## Milestone 2: Android as a native extended Windows display — active

Product acceptance definition: a trusted Android phone or tablet appears as a real monitor in Windows Settings → System → Display. The user can choose **Extend**, position the display, and move normal application windows onto it. A mirrored desktop preview or a video surface alone does not satisfy this milestone.

Architecture and acceptance gates: [docs/second-screen-architecture.md](second-screen-architecture.md).

### Windows virtual display and control plane
- [x] Select Microsoft IddCx / Indirect Display Driver (UMDF) as the Windows virtual-monitor architecture.
- [x] Define separation between the Rust/egui desktop UI, Bridge Display Windows service, and C++ UMDF display driver.
- [x] Add the WDK/Visual Studio C++ IddCx driver project based on Microsoft's official sample.
- [x] Build and package the x64 IddCx driver in GitHub Actions.
- [x] Replace sample demonstration monitors with a Bridge-branded virtual monitor and conservative portrait mode list.
- [x] Add restricted driver-side connect/disconnect/status control IOCTLs; monitor arrival is no longer automatic at adapter startup.
- [x] Add the Rust Windows display service, bounded versioned named-pipe protocol, and driver QUERY/CONNECT/DISCONNECT client.
- [x] Implement the authenticated desktop-client identity check, Android peer attestation verification, short-lived display lease, and lease-bound driver operations in the development service path.
- [x] Add a desktop diagnostic query for live driver connection state and supported display-mode count.
- [ ] Validate driver installation, service registration, virtual-monitor arrival/removal, and safe mode negotiation on a physical Windows test machine.
- [ ] Finalize least-privilege service configuration, recovery policy, and elevated installer lifecycle.
- [ ] Complete and validate signed driver distribution and the user-facing trust/install procedure.
- [ ] Package driver + service installation, upgrade, rollback, and removal safely with explicit elevation.

### Windows capture and media sender
- [ ] Acquire real IddCx Direct3D swap-chain frames without blocking display composition.
- [x] Implement the persistent Windows Media Foundation H.264 encoder and send bounded encoded access units over the authenticated Bridge session.
- [x] Preserve encoder presentation timestamps and validate bounded access-unit fragmentation/reassembly.
- [x] Retain the BGRA frame path as a fallback when H.264 setup or encoding fails.
- [x] Recreate the encoder when display dimensions change and request a fresh GOP after Android decoder recovery requests.
- [ ] Confirm the first output after encoder recreation is a valid IDR with the required SPS/PPS on target Windows hardware.
- [ ] Add measured adaptive bitrate, frame pacing, and congestion handling.
- [ ] Evaluate a dedicated low-latency media transport separately from the current authenticated session transport.

### Android second-screen renderer
- [x] Receive and reassemble bounded H.264 access units on the authenticated session.
- [x] Add the Android MediaCodec AVC SurfaceView renderer with SPS/PPS configuration and presentation timestamps.
- [x] Add bounded decode work, frame-sequence gap detection, and keyframe-based recovery requests.
- [x] Add keyframe-request throttling and preserve the BGRA fallback path.
- [x] Handle renderer teardown/reconfiguration and dynamic frame dimensions in the implementation.
- [ ] Validate sustained H.264 playback on the target Redmi Note 7 Pro.
- [ ] Validate rotation, orientation changes, screen sleep/wake, Wi-Fi recovery, and decoder recreation on hardware.
- [ ] Complete the end-to-end Windows Extend-mode experience with real application windows on the Android display.

### Input, reliability, and performance
- [x] Define a bounded pointer-event wire format inside the authenticated session (protocol foundation only).
- [ ] Bind touch/pen input to the active display lease and virtual-monitor dimensions; enforce ordering and rate limits.
- [ ] Inject input through a narrowly scoped Windows API and release active pointer state safely on disconnect.
- [ ] Measure end-to-end latency, frame pacing, bitrate, CPU/GPU use, and thermal impact on physical devices.
- [ ] Test display hotplug, Windows sleep/resume, lock/unlock, GPU reset, and service/driver recovery.
- [ ] Test network loss/roaming and Android rotation/process lifecycle.
- [ ] Verify safe uninstall and that the physical display remains usable after driver/service failure.

**Current hardware-validation gate:** the CI build pipeline and software streaming components are in place, but CI does not prove that the driver installs, the virtual monitor appears, or H.264 video plays on real hardware. Those checks must be completed on the Windows PC and target Android handset before calling Second Screen usable.

## Milestone 3: secure local file transfer — planned; shared foundation implemented

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

## Other device capabilities
- [ ] Capability permission model
- [x] Explicit text sharing between connected trusted Windows and Android devices (16 KiB cap; opt-in; no background clipboard monitoring)
- [ ] Automatic clipboard monitoring/sync (deferred for Android background-access restrictions and additional privacy controls)
- [ ] Local file transfer with explicit destination approval (planned under Milestone 3)
- [ ] Presence and device status
- [ ] Android notification bridge (subject to platform permissions)
- [ ] iOS-compatible feature set using public platform APIs

## Release quality
- [ ] Signed Windows installer and supported driver trust/install flow
- [ ] Signed Android release bundle
- [ ] Apple signing and TestFlight distribution
- [ ] End-to-end acceptance tests across physical devices
