# Bridge Second Screen Architecture

## Product requirement

Bridge Second Screen turns a trusted Android phone or tablet into a **real extended Windows display**. Windows must enumerate it as another monitor in Settings > System > Display. The user can choose Extend, move application windows onto it, and arrange its position relative to physical monitors.

This is not desktop mirroring, a remote-control window, Miracast casting, or a video player that happens to show the desktop. The virtual display must exist in the Windows display topology.

## Windows display architecture

Bridge will **reuse the upstream VirtualDrivers Virtual Display Driver (VDD)**. Bridge will not maintain, compile, or sign its own IddCx/UMDF driver.

- Initial candidate: **Parsec Virtual Display Driver 0.45** via the MIT-licensed `nomi-san/parsec-vdd` integration project.
- The newer VirtualDrivers package was rejected for Bridge's x64 target: its 25.7.23 bundle contained only ARM64 and x86 driver packages, while its older x64 ZIP contained a catalog that Windows reported as `NotSigned` and a certificate-install batch file. Bridge will not install that self-signed certificate.
- Parsec VDD's upstream project documents Windows 10/11 64-bit support and a signed driver package. Its companion ParsecDisplay application is unsigned; Bridge will not bundle or execute that companion app.
- The Parsec VDD driver must remain unmodified. CI verified the x64 INF/DLL/catalog and a valid catalog signature from Microsoft Windows Hardware Compatibility Publisher. The pinned portable archive SHA-256 is `9792e4121d85ed3e4c40c2d8ba36ec8657e13227a8357d719d923e801238ccdd`.
- No test-signing mode, Secure Boot change, Bridge-owned driver certificate, or certificate-root import is permitted.

### Required Bridge integration

VDD creates and manages the Windows virtual monitor. It does **not** automatically deliver frames into Bridge's authenticated video pipeline. Bridge must implement a separate capture adapter:

1. Identify the VDD-created display using Windows display topology APIs (not a hard-coded display index).
2. Capture only that virtual output using a supported Windows capture API. Prototype DXGI Desktop Duplication first; evaluate Windows Graphics Capture if the virtual output is not exposed reliably.
3. Convert/crop/scale captured frames into the existing bounded frame pipeline and Media Foundation H.264 encoder.
4. Send encoded frames through the existing authenticated Bridge session to Android's MediaCodec renderer.
5. Keep capture tied to the active, attested Android session; stop capture and disable/remove the virtual output on disconnect or lease expiry.
6. Validate monitor hotplug, mode changes, GPU reset, sleep/resume, multi-monitor layouts, and capture recovery.

**No mirror-only fallback counts as completion.** The acceptance gate remains: Windows Settings > System > Display lists the VDD monitor, the user selects Extend, and ordinary application windows can be moved onto the Android display.

### Third-party driver lifecycle and licensing

- Pin the upstream release and its archive hash; do not download an unpinned "latest" package at install time.
- Preserve upstream copyright and license notices in Bridge's third-party notices.
- Verify the selected release's redistribution terms and exact signed package contents before embedding it in Bridge's installer. MIT licensing of the source repository does not, by itself, prove that every bundled binary or companion application can be redistributed.
- Install the vendor-supplied signed driver package through an elevated, rollback-capable installer. Do not install it by importing a Bridge-generated certificate or re-signing the catalog.
- Provide clean uninstall and recovery if driver installation fails. The Parsec VDD API requires a keepalive ping roughly once per second or the virtual display is removed. Bridge's service must own that watchdog and stop it cleanly. The upstream Rust client reference is https://github.com/rohitsangwan01/parsec-vdd-rust; review its API/unsafe code before reuse rather than assuming it is production-ready. Bridge must also document safe recovery and known limitations, including no HDR.
- Keep the current Bridge IddCx implementation only as a temporary migration reference. It must not be built, signed, installed, or shipped in the new path.

## Windows application and service split

Keep the existing Rust + egui Bridge desktop application. Do not turn the entire GUI into a privileged process.

Introduce separate components:

1. **Bridge desktop UI (existing Rust/egui app):** normal interactive user-session app. Owns device selection, permissions, display settings, connect/disconnect actions, status, and diagnostics.
2. **Bridge Display Service (Windows service):** privileged boundary for driver lifecycle, authenticated session coordination, media transport, encoder orchestration, recovery, and Windows service startup/shutdown. Expose a narrow, ACL-protected named-pipe API to the desktop UI. Run with the least privilege compatible with driver communication; do not default to LocalSystem without a demonstrated need.
3. **Upstream Parsec Virtual Display Driver (third-party signed package):** owns the virtual display adapter, monitor PnP lifecycle, and mode negotiation. Bridge does not build or sign this driver. Bridge's capture adapter reads the resulting virtual output through Windows display/capture APIs.

Driver installation and device configuration require elevation. Normal use of the desktop UI should not require running the whole app as administrator. Package the driver and service with the Windows installer only after installation, upgrade, rollback, and uninstall behaviour are defined.

### Display service control boundary (migration in progress)

The Rust service in `apps/windows-service` exposes a versioned local named-pipe protocol (`\\.\pipe\BridgeDisplayService`). The unprivileged desktop client uses that pipe for Android attestation, short-lived lease renewal, driver connect/disconnect, frame reads, and read-only driver status queries. The service attributes each pipe client to a live process image, verifies the configured desktop publisher, verifies the Android signature against the pairing-pinned key, and binds the resulting 15-second lease to that process and Android fingerprint. Driver mutations and frame reads require the exact active lease. The pipe is local-only and ACL-protected; the driver interface remains restricted to LocalSystem and local administrators.

The desktop Second Screen panel currently exposes a read-only **Refresh monitor status** action for the legacy Bridge driver. It must be updated to report the upstream VDD device and actual Windows display topology. It does not bypass attestation or change driver state.

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

The Android receiver reassembles bounded BGRA8 and H.264 access-unit fragments on the authenticated session reader. Completed frames are coalesced before main-thread dispatch: at most one UI callback is pending per session, and a newer complete frame replaces an older pending frame. The encoded-video path also detects gaps in completed access-unit sequences. The Android AVC renderer uses MediaCodec and SurfaceView with SPS/PPS configuration, a bounded decode worker, keyframe-based recovery, and BGRA fallback. Physical-device playback and end-to-end latency remain unverified.

## Android frame-rate diagnostics

The Android second-screen view estimates incoming frame rate from completed-frame arrival times on the device's monotonic clock. The displayed value uses a smoothed estimate to make short-term Wi-Fi jitter easier to interpret. It measures frame arrivals, not decoder presentation cadence or end-to-end latency; those require timestamp synchronization and the future encoded media pipeline.

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


## Android stalled-frame detection

The Android second-screen UI tracks the age of the most recently completed frame using Android's monotonic elapsed-realtime clock. If no newer frame arrives for 1.5 seconds, both the compact preview and immersive view report a stalled stream and show the elapsed time since the last frame. The last rendered image remains visible for diagnosis; the warning does not claim that the authenticated device session has disconnected. A new frame immediately clears the warning. This is an arrival watchdog, not a decoder-health or end-to-end latency measurement.


## Windows frame-sender diagnostics

The Windows desktop sender reports a rolling one-second source-side estimate while frames are successfully delivered through the authenticated session: sent FPS, raw BGRA payload bitrate, the number of capture sequence values superseded before the sender could read them, and average time spent sending each complete frame. The bitrate is raw pixel payload before Noise/TCP framing and is not a measured Wi-Fi wire rate. Superseded sequence values indicate latest-frame replacement between sender polls; they are not evidence of network packet loss. Compare this source estimate with Android's incoming-frame estimate to localize capture, sender, network, and receiver bottlenecks.


## Android H.264 decoder capability probe

The Android Second Screen page inspects the platform's regular (non-encoder) MediaCodec list for H.264/AVC decoders. It reports the preferred declared decoder, hardware-acceleration classification where Android exposes it, maximum declared dimensions/frame rate/bitrate, and whether 1080 × 2340 at 30 FPS is declared supported. The probe does not instantiate or start a codec and does not prove that a decoder can render a live stream to the eventual SurfaceView. Runtime configure/start, color format, sustained playback, rotation, and teardown must be validated by the MediaCodec renderer implementation and on physical hardware.


## Bounded H.264 access-unit wire format

The shared transfer crate defines a dedicated packet discriminator for encoded H.264/AVC access-unit fragments, separate from the existing raw BGRA frame tag. Each fragment carries a monotonic frame sequence, dimensions, source timestamp, total access-unit length, byte offset, and keyframe flag. Fragment payloads remain within the existing encrypted-message limit; a complete encoded access unit is capped at 16 MiB. The assembler accepts one access unit at a time, validates contiguous offsets and consistent metadata, and rejects overlapping or stale sequences. This is a protocol foundation only: it does not yet replace the Windows BGRA sender, establish a dedicated datagram media transport, or connect Android MediaCodec to a Surface.


## Android encoded-video packet reception

The Android authenticated-session reader recognizes the H.264 access-unit packet tag separately from raw BGRA frames. It reassembles one access unit at a time with a 16 MiB cap, validates metadata, keyframe flag, contiguous fragment offsets, and strictly increasing completed sequences, then exposes the completed access unit and source metadata to the connection runtime. Session teardown clears the latest encoded frame for that peer. The access unit is not yet passed to MediaCodec; decoder configuration, input-buffer scheduling, SurfaceView rendering, and sender-side H.264 encoding remain separate milestones.


## Windows Media Foundation encoder foundation

The Windows desktop stream lifecycle now initializes Media Foundation before starting its frame worker and owns a guard that pairs each successful startup with one shutdown. Initialization failure is reported and does not silently claim that hardware encoding is available. The current sender continues using the existing BGRA packet path; H.264 encoder MFT discovery/activation, GPU-surface input, rate control, and encoded access-unit transmission are subsequent steps. This milestone validates only runtime initialization, not encoder availability or encoded streaming.


## Windows Media Foundation encoder discovery

The desktop stream startup now enumerates registered Media Foundation video-encoder transforms that advertise H.264 output after platform initialization and reports the discovered names in the second-screen status. Enumeration requests hardware, synchronous, asynchronous, and local transforms. The current result is diagnostic only: it does not yet prove a transform accepts Bridge's target NV12 dimensions/rate, does not activate an encoder, and does not emit H.264 access units. BGRA remains the active transport fallback.


## Windows H.264 encoder activation probe

The encoder diagnostic now attempts to activate each registered H.264-output MFT as an IMFTransform and query its input/output stream counts. Activation failures are retained per candidate rather than aborting discovery of other encoders. Successful transform objects are released and shut down immediately; no frame types are negotiated and no samples are submitted. This confirms activation only, not NV12 input support, hardware acceleration, or successful H.264 bitstream production. BGRA remains the active sender.


## Windows NV12-to-H.264 profile negotiation probe

For each successfully activated H.264-output transform, the desktop now constructs test-only Media Foundation media types for NV12 input and H.264 output at 1080×2340, 60 fps, and an 8 Mbps target bitrate. It calls the transform's output/input type setters with MFT_SET_TYPE_TEST_ONLY, so no active type is committed and no frames are processed. Diagnostics distinguish activation from acceptance of this exact target profile. Hardware acceleration is still not inferred from registration or friendly names; actual encoding and bitstream output remain future steps.


## Windows committed H.264 encoder profile probe

The Windows diagnostic now goes beyond test-only negotiation: for transforms accepting the target profile, it commits H.264 output first and NV12 input second, then reads back the current input/output frame dimensions. Per-candidate results distinguish profile acceptance from successful committed configuration. The temporary transform is still shut down after the probe and no samples are submitted, so this does not yet produce encoded video or switch the sender away from BGRA.


## First Windows H.264 encoder sample probe

When the first captured 1080×2340 BGRA frame arrives, the Windows stream worker converts a copy to bounded NV12 and attempts to submit exactly one sample to a compatible synchronous Media Foundation H.264 encoder. The worker initializes its own COM multithreaded apartment; the probe negotiates the configured 60 fps profile, sends begin/start-streaming messages, attaches the NV12 buffer with a 100 ns timestamp and frame duration, and requests an output sample using the transform's advertised allocation model. A returned output is checked against the 16 MiB access-unit bound. A transform that accepts the sample but needs more input is reported separately from one that produces output.

This is a one-sample diagnostic only. The transform is shut down after the probe, encoded bytes are not transmitted, asynchronous MFTs are deliberately excluded, and the existing authenticated BGRA sender remains active. If Media Foundation startup, COM initialization, conversion, activation, or encoding fails, BGRA streaming continues. Sustained encoding, keyframe/configuration handling, persistent encoder lifetime, H.264 packet transmission, and Android MediaCodec rendering remain future milestones.

## BGRA-to-NV12 conversion foundation

The shared Rust transfer crate now provides a bounded CPU conversion from tightly packed BGRA8 display frames to NV12 using BT.709 video-range luma/chroma coefficients. It validates frame metadata, requires even dimensions, averages chroma over each 2×2 pixel block, preserves sequence/timestamp metadata, and caps output at the existing 64 MiB frame limit. Cross-platform unit tests cover black/white levels, chroma subsampling, and malformed/odd-sized inputs. This is a conversion primitive only: the Windows sender still transmits BGRA, and no Media Foundation input samples are submitted yet.


## Android MediaCodec Surface renderer foundation

The Android second-screen UI owns a bounded, single-threaded AVC decode worker and a native SurfaceView. It configures MediaCodec for the dimensions carried by authenticated H.264 access units, supplies SPS/PPS as codec-specific data, queues presentation timestamps in microseconds, and releases decoded output buffers to the Surface. The worker queue is bounded to two pending frames. Surface size changes and destruction clear stale queued work, release the codec on its worker, and return visibility to the BGRA fallback. Decoder errors and frame-sequence gaps request a fresh keyframe from Windows.

The Windows session reader accepts a fixed-size keyframe-request message only inside the existing authenticated Noise session. The stream worker coalesces requests into an atomic flag and restarts its encoder instance to begin a fresh GOP. The persistent encoder is created for the actual virtual-display dimensions and recreated when those dimensions change; NV12/H.264 media types are negotiated against that size rather than assuming 1080×2340. Encoder reinitialization and keyframe output behavior still require validation with real Windows hardware and an Android device. The BGRA path remains available throughout.


## Android AVC decoder configuration

Before creating the MediaCodec decoder, the Android renderer now extracts Annex-B SPS and PPS NAL units from the first IDR access unit and supplies them as `csd-0` and `csd-1` in the decoder format. It waits for a keyframe containing both parameter sets rather than starting a decoder with incomplete codec configuration. The parser retains the Annex-B start codes and is covered by JVM unit tests for three-/four-byte start codes, missing parameter sets, non-Annex-B input, and trailing zero bytes. This improves decoder initialization but does not yet guarantee that every Media Foundation encoder emits SPS/PPS with every recovery keyframe or solve packet-loss/keyframe requests.


## Preserve H.264 presentation timestamps

The persistent Windows Media Foundation encoder now reads the presentation timestamp from each emitted output sample instead of assigning the timestamp of whichever captured frame happened to trigger the drain. Encoded access units carry the encoder's 100 ns timestamp through the desktop sender and convert it to nanoseconds for the Android packet protocol. This preserves timestamps when the encoder buffers or reorders output. If an output sample has no timestamp, the H.264 encode path reports an error and retains the BGRA fallback rather than silently attaching a potentially incorrect timestamp. Physical-device A/V timing and encoder-specific timestamp behavior still require validation.


## H.264 recovery and dynamic display dimensions

Android detects missing access-unit sequence numbers both before main-thread dispatch and inside the decoder after callback coalescing. It sends a bounded 9-byte recovery request over the authenticated Noise session. Windows validates the discriminator and packet length, coalesces requests, and recreates the per-session Media Foundation encoder so the next GOP starts from a fresh encoder state. Android waits for a configured IDR before resuming decode. The request carries the last observed sequence for diagnostics; it does not create a separate socket or trust path.

The Windows persistent encoder now negotiates NV12 input and H.264 output for the current even-sized virtual-display dimensions (bounded to 2–4096 in each axis). A display-size change tears down and recreates the encoder on the existing COM/MF worker. If a dimension is unsupported or reconfiguration fails, H.264 is disabled for that attempt and the BGRA stream continues. This is software/CI coverage only: actual encoder IDR behavior, rotation transitions, and playback must still be checked on the target Windows host and Android handset.
