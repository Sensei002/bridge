<p align="center">
  <img src="assets/Bridge%20%E2%80%94%20Connect,%20Share,%20Beyond.png" alt="Bridge — Connect, Share, Beyond" width="760" />
</p>

<h1 align="center">Bridge</h1>
<p align="center"><strong>CONNECT · SHARE · BEYOND</strong></p>
<p align="center">One ecosystem. Every device.</p>

Bridge is an open-source, local-first device interoperability project for Windows, Android, and iOS. The goal is to make nearby devices work together through one secure, permission-based ecosystem.

> **Current status — Milestone 2: Android as a real extended Windows display (in progress).** The Windows IddCx driver and Bridge Display Service build in CI; the Windows sender and Android H.264/MediaCodec receive-and-render path, dynamic sizing, and keyframe recovery are implemented in the development code. The next gate is physical validation: install and activate the virtual display on Windows, confirm it appears in Display Settings, connect the Android device, and verify that Windows Extend mode renders real application windows on the phone. CI success does not prove physical monitor installation or live playback. Milestone 1 authenticated Windows ↔ Android pairing is complete; iOS and Linux physical-device validation remain deferred.

## Platforms

- **Windows:** Native Rust + [egui](https://github.com/emilk/egui) desktop application. No WebView2, Tauri, Electron, Chromium, or embedded browser runtime.
- **Android:** Kotlin + Jetpack Compose.
- **iOS:** SwiftUI native app.
- **Shared engine:** Rust workspace with versioned protocol types, device registry, pairing consent state machine, local-network discovery, encrypted-session primitives, and trust persistence.

## Current focus: Milestone 2 — Second Screen

Bridge Second Screen is intended to make an Android phone or tablet a **real extended Windows monitor**. Windows must enumerate it in Settings → System → Display, allow the user to select Extend, and let application windows move onto the Android display. It is not desktop mirroring or a remote-control preview.

Current implementation includes the IddCx/UMDF driver project, a separate Windows display service with an authenticated control boundary, Windows H.264 frame sending, Android H.264 access-unit reception and MediaCodec Surface rendering, dynamic display-size handling, and keyframe recovery. BGRA remains a fallback path.

Still requiring real-device validation or further implementation: driver/service installation and monitor arrival on a physical Windows host, Direct3D swap-chain frame acquisition from the virtual display, actual H.264 playback and recovery on the target hardware, orientation/rotation, latency and sustained performance, and the full Windows Extend-mode acceptance test. Android-to-Windows touch input is not yet implemented end to end.

See [docs/roadmap.md](docs/roadmap.md) for task status and [docs/second-screen-architecture.md](docs/second-screen-architecture.md) for architecture and acceptance gates.

## Milestone 1: authenticated local connectivity — complete for Windows + Android

Implemented foundations:

- mDNS/DNS-SD discovery on Windows, Android NSD, and iOS Bonjour.
- Versioned, validated discovery metadata treated as untrusted until authentication.
- Bounded length-prefixed TCP framing, socket timeouts, and Noise XX sessions.
- SHA-256 static-key fingerprints, symmetric six-digit short authentication strings, and pinned-identity checks.
- Device identity persistence using Windows Credential Manager, Android Keystore-backed storage, and iOS Keychain.
- Explicit approval on the receiving device before first-time trust; the initiating device is trusted only after receiving the encrypted acceptance response. Persistent trusted-device records and revocation.
- Authenticated descriptor exchange, replay-resistant transport counters, encrypted pairing-decision messages, and one-tap approval on the receiving device.

Milestone 1 validation outcome:

- **Windows ↔ Android:** physical-device pairing and reconnection validated in both directions. App restart/reconnection, Wi-Fi disconnect/reconnect, Windows firewall, and Android network-permission behaviour were tested and reported working.
- **iOS:** CI Simulator build passes; physical-device interoperability remains unverified because no iPhone or Mac test environment is currently available.
- **Linux:** deferred until a Linux installation is available for testing.
- **Follow-up hardening:** broader router/AP-isolation and IPv6-only coverage, resource-limit/lifecycle stress testing, and an independent security review remain recommended before a broad public release.

See [docs/protocol.md](docs/protocol.md), [docs/security-model.md](docs/security-model.md), [CONTRIBUTING.md](CONTRIBUTING.md), and [SECURITY.md](SECURITY.md).

## Milestone 3: secure local file transfer — planned / foundations implemented

The shared Rust transfer engine already provides bounded transfer-control messages, 48 KiB chunk framing, file-offer validation, a 2 GiB initial size ceiling, SHA-256 integrity verification, safe filename validation, collision-safe publication, and temporary-file cleanup. Wiring it into the authenticated desktop and Android sessions, approval UI, progress/cancellation, and physical-device transfer tests remain future work. This is not the current active milestone.

## Repository layout

- `crates/bridge-protocol` — versioned wire types.
- `crates/bridge-core` — device registry, pairing lifecycle, and QR invitation validation.
- `crates/bridge-security` — Noise XX handshake and authenticated encrypted sessions.
- `crates/bridge-network` — mDNS/DNS-SD discovery, bounded TCP framing, and Noise transport.
- `crates/bridge-mobile` — C ABI/JNI FFI for native mobile clients.
- `apps/desktop` — native Rust + egui Windows desktop app.
- `apps/android` — Android app.
- `apps/ios` — iOS app.
- `assets/Bridge — Connect, Share, Beyond.png` — original master brand artwork.
- `scripts/generate_brand_assets.py` — deterministic platform icon generation from the master artwork.
- `docs/` — architecture, protocol, security model, and roadmap.
- `.github/workflows/` — CI and release automation.

## CI/CD

All compilation, tests, packaging, and releases run in GitHub Actions. No local build is required.

The workflows validate the Rust workspace, native Windows desktop app, Windows display service, Windows IddCx driver package, Android APK, and iOS Simulator app as configured for each workflow. Development-channel releases publish Windows and Android assets with an update manifest. A successful CI build is not a substitute for physical-device acceptance testing.

**Signing note:** Windows driver and trusted-binary signing runs are part of the trusted GitHub Actions pipeline. Do not treat a build artifact as install-ready until the signed output and trust/install procedure have completed verification. The private signing key must never be included in releases. Android release signing is configured independently; iOS archive signing is not configured. See [Windows signing and trust plan](docs/windows-code-signing.md).

## Windows desktop technology

The Windows client is a native Rust desktop application rendered by egui. It does not embed a browser engine and does not depend on Microsoft Edge WebView2 Runtime. The desktop build compiles directly from the Rust crate in `apps/desktop`.

## Security principles

- Pairing must require user approval on both devices.
- Noise XX provides an authenticated key exchange and encrypted transport; the remote static-key fingerprint must be checked before a peer is trusted.
- Local-network discovery is only a way to find peers, not proof of identity. Noise XX authenticates the encrypted session; first-time pairing uses trust-on-first-use after the receiving device explicitly approves the authenticated peer. Accept only devices you recognize.
- Device identities must be revocable.
- Capabilities are independently permissioned.
- Network messages must be versioned, validated, and size-bounded.
- No silent cloud upload or background relay.
- Never commit secrets or signing keys.

A successful CI build or loopback test alone is not proof of real-device interoperability. See the roadmap for the current acceptance gates.

## License

MIT. See [LICENSE](LICENSE).
