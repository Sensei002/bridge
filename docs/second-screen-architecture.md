# Bridge Second Screen Architecture

## Product requirement

Bridge Second Screen turns a trusted Android phone or tablet into a **real extended Windows display**. Windows must enumerate it as another monitor in Settings > System > Display. The user can choose Extend, move application windows onto it, and arrange its position relative to physical monitors.

This is not desktop mirroring, a remote-control window, Miracast casting, or a video player that happens to show the desktop. The virtual display must exist in the Windows display topology.

## Windows display architecture

Use Microsoft's **Indirect Display Driver (IDD) model through IddCx**.

- Build a dedicated Windows Indirect Display Driver using the Windows Driver Kit (WDK), C++, UMDF, and IddCx.
- The driver creates the indirect display adapter without an attached monitor. A restricted IddCx IOCTL control plane accepts connect, disconnect, and status operations from the privileged Bridge Display Service; the service will issue arrival only for a trusted Android display session and departure on disconnect.
- Consume the IddCx swap-chain frames using Direct3D. Keep frame processing bounded and non-blocking; do not perform network waits or expensive encoding work on the swap-chain callback thread.
- The driver is a **user-mode UMDF driver**, not a custom kernel-mode graphics driver. Microsoft explicitly documents that IDD uses UMDF and does not support kernel-mode components. It receives the desktop image as a DirectX surface suitable for processing/encoding.
- Do not inject into DWM, hook games, scrape the desktop with GDI, or implement a display miniport from scratch.

The official Microsoft IddSampleDriver is the starting reference for adapter/monitor lifecycle, mode negotiation, swap-chain assignment, and frame processing. It is a sample, not production-ready code; its INF and frame-processing loop need real implementation and review.

## Windows application and service split

Keep the existing Rust + egui Bridge desktop application. Do not turn the entire GUI into a privileged process.

Introduce separate components:

1. **Bridge desktop UI (existing Rust/egui app):** normal interactive user-session app. Owns device selection, permissions, display settings, connect/disconnect actions, status, and diagnostics.
2. **Bridge Display Service (Windows service):** privileged boundary for driver lifecycle, authenticated session coordination, media transport, encoder orchestration, recovery, and Windows service startup/shutdown. Expose a narrow, ACL-protected named-pipe API to the desktop UI. Run with the least privilege compatible with driver communication; do not default to LocalSystem without a demonstrated need.
3. **Bridge Indirect Display Driver (C++/UMDF/IddCx):** owns the virtual display adapter, monitor PnP lifecycle, mode negotiation, and Direct3D swap-chain acquisition. It must not own UI, pairing consent, or Android application logic.

Driver installation and device configuration require elevation. Normal use of the desktop UI should not require running the whole app as administrator. Package the driver and service with the Windows installer only after installation, upgrade, rollback, and uninstall behaviour are defined.

### Display service control boundary (implemented development path)

The Rust service in `apps/windows-service` exposes a versioned local named-pipe protocol (`\\.\pipe\BridgeDisplayService`). The unprivileged desktop client uses that pipe for Android attestation, short-lived lease renewal, driver connect/disconnect, frame reads, and read-only driver status queries. The service attributes each pipe client to a live process image, verifies the configured desktop publisher, verifies the Android signature against the pairing-pinned key, and binds the resulting 15-second lease to that process and Android fingerprint. Driver mutations and frame reads require the exact active lease. The pipe is local-only and ACL-protected; the driver interface remains restricted to LocalSystem and local administrators.

The desktop Second Screen panel now exposes a read-only **Refresh monitor status** action. It reports whether the IddCx driver currently has a virtual monitor connected and how many modes the driver advertises. It does not bypass attestation or change driver state.

The service is packaged with the per-user MSI as an executable and administrator-run setup scripts. The MSI does not silently register a privileged service. Installation remains an explicit elevated action, with Authenticode verification enabled by default. This is still a development integration path: successful CI builds do not establish that a physical Windows machine has the driver and service correctly installed or that a monitor appears in Windows Display Settings. The detailed publisher-verification, one-time Noise-session challenge, ephemeral attestation, lease-renewal, replay-protection, and fail-closed requirements remain documented in [Windows Display Service Authentication Design](windows-service-authentication.md).

## Media pipeline

Target a low-latency hardware-assisted pipeline:

1. Windows composes the extended desktop onto the virtual monitor.
2. IddCx supplies Direct3D swap-chain surfaces.
3. A dedicated frame pipeline uses GPU surfaces and hardware video encoding where available (Media Foundation hardware MFT first; vendor-specific paths only if justified). Avoid CPU readback/copy on every frame.
4. Send timestamped, sequenced video frames over a dedicated real-time media transport, separate from file-transfer traffic. Do not put a continuous high-frame-rate video stream behind the existing TCP file-transfer queue.
5. Authenticate the media peer against the already trusted Bridge device identity. Reuse established cryptographic primitives or a standard authenticated transport; do not invent a new encryption scheme.
6. Android receives the stream using a hardware decoder (MediaCodec) and renders to a dedicated full-screen SurfaceView/Surface. Handle rotation, display sleep, decoder recreation, and Wi-Fi changes.

Start with H.264 hardware encode/decode for broad Android compatibility. Evaluate HEVC/AV1 only after a stable baseline. Adapt resolution, frame rate, and bitrate to measured network and device capability. Prefer dropping stale frames over building latency queues.

## Display input transport foundation

The shared transfer protocol now defines a fixed-size, bounded pointer sample for the reverse path from Android to Windows. Each event carries a monotonically increasing sequence, monotonic timestamp, pointer ID (0–9), action (down/move/up/cancel), and normalized 16-bit coordinates. The packet is transported only inside the already-authenticated Noise session; it does not introduce a new socket or trust boundary.

This is a wire-format foundation only. It does not yet inject input into Windows, authenticate a display lease for input operations, or expose touch/stylus controls in the Android UI. The next implementation must bind input to the active display lease and virtual-monitor dimensions, enforce event ordering/rate limits, and use a narrowly scoped Windows input API. Disconnect, lease expiry, malformed packets, and stale sequences must fail closed and release any active pointer state.

## Android frame delivery status

The Android receiver reassembles bounded BGRA8 frame fragments on the authenticated session reader. Completed frames are coalesced before dispatch to the main thread: at most one UI callback is pending per session, and a newer complete frame replaces an older pending frame. Completed frame sequence numbers must increase monotonically. This prevents a slow UI from accumulating stale frame callbacks, but the current renderer still converts BGRA8 frames into Compose bitmaps on the UI side. A dedicated MediaCodec/SurfaceView renderer and hardware video codec pipeline remain future work.

## Android bitmap conversion

The Compose second-screen view performs bitmap allocation and BGRA8 pixel upload on a cancellable coroutine using `Dispatchers.Default`, rather than doing the CPU work on the main thread. A new frame key cancels the previous conversion coroutine so obsolete conversion results are not applied to the UI. This reduces main-thread work but is still a CPU bitmap path; it is not a hardware-decoded video renderer and does not eliminate frame copies.

## Initial device profile and modes

The first physical test target is the currently paired Redmi Note 7 Pro. Start with a portrait-native 1080 x 2340 mode at 60 Hz, plus a lower-bandwidth fallback mode. Confirm actual supported modes and decoder capability at runtime rather than assuming every Android device supports the same profile. Rotation and landscape modes must be negotiated explicitly.

## Delivery phases and acceptance gates

### Phase A — driver and build foundation
- Add a WDK/Visual Studio C++ UMDF IddCx driver project under a dedicated Windows driver directory.
- Add a GitHub Actions Windows runner job that builds the driver package and validates the INF/package outputs. Keep the existing Windows MSI, Android APK, Rust, and iOS CI jobs intact.
- Start from the official Microsoft IddSampleDriver structure; record the exact WDK/SDK and IddCx versions.
- [x] Implement driver-side connect/disconnect/status IOCTLs and defer monitor arrival until explicitly requested by the service.
- [x] Implement the Bridge Display Service control client, service identity/ACL, and authenticated-session-to-monitor lifecycle binding in the development path.
- [x] Add a desktop diagnostic query for live driver connection state and supported display-mode count.
- [ ] Physical acceptance: on a dedicated Windows test machine, Device Manager and Windows Display Settings show the Bridge virtual monitor, and connect/disconnect adds/removes it without destabilizing the physical display.

### Phase B — real frame acquisition and local preview
- Implement swap-chain lifecycle and Direct3D surface handling.
- Add a local diagnostic preview/recording path to validate frame delivery, timestamps, mode changes, and device loss before networking.
- Acceptance: frames arrive continuously with bounded resource use; no UI stalls or swap-chain callback blocking.

### Phase C — encrypted low-latency media transport
- Add a dedicated media channel tied to the trusted Bridge identity, with sequence numbers, timestamps, bounded jitter buffering, and adaptive bitrate/frame pacing.
- Keep file transfer, heartbeat, pairing, and control traffic independent.
- Acceptance: packet loss does not create an ever-growing delay; reconnect and Wi-Fi loss cleanly pause/resume the virtual monitor.

### Phase D — Android monitor renderer
- Add a dedicated Second Screen Android experience using MediaCodec and SurfaceView, with immersive mode, rotation handling, connection state, and safe decoder teardown.
- Acceptance: Windows Extend mode places windows on the phone display; the Android screen shows the extended desktop, not a duplicate of the primary display.

### Phase E — production installer and reliability
- Install/update/remove the driver and service safely through the MSI with explicit elevation and rollback.
- Establish a supported driver signing and distribution process before asking ordinary users to install it. Development test-signing must be clearly separated from production releases.
- Test sleep/resume, lock/unlock, GPU reset, Android process/service restart, Wi-Fi roam, device rotation, multiple GPUs, and uninstall with the virtual monitor disconnected.

## Performance and safety requirements

- Optimize for end-to-end latency, not just maximum FPS.
- Use hardware encoding/decoding and avoid per-frame CPU copies where possible.
- Keep queues bounded; discard obsolete frames under congestion.
- Never let a slow phone or network block Windows display composition.
- Remove the virtual monitor on explicit disconnect and recover safely after service/driver failure.
- Require a trusted, authenticated Bridge peer before exposing the virtual monitor stream.
- Do not require a custom kernel-mode graphics driver. The supported IDD/IddCx model is UMDF user mode.

## Primary references

- Microsoft, [Indirect Display Driver Model Overview](https://learn.microsoft.com/en-us/windows-hardware/drivers/display/indirect-display-driver-model-overview)
- Microsoft, [Windows Driver Samples — Indirect Display Driver](https://github.com/microsoft/Windows-driver-samples/tree/main/video/IndirectDisplay)
- Microsoft sample README notes that the sample only enumerates a monitor and that its INF and swap-chain processing need production work.
