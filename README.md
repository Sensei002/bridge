<p align="center">
  <img src="assets/Bridge%20%E2%80%94%20Connect,%20Share,%20Beyond.png" alt="Bridge — Connect, Share, Beyond" width="760" />
</p>

<h1 align="center">Bridge</h1>
<p align="center"><strong>CONNECT · SHARE · BEYOND</strong></p>
<p align="center">One ecosystem. Every device.</p>

Bridge is an open-source, local-first device interoperability project for Windows, Android, and iOS. The goal is to make nearby devices work together through one secure, permission-based ecosystem.

> **Status: Milestone 1 complete for the validated Windows + Android pairing target.** Both platforms have passed real-device discovery, bidirectional pairing, receiver approval, trusted reconnection, app restart/reconnection, and local-network reconnect checks. Windows firewall and Android network permissions have been tested successfully. iOS builds in CI but has not been physically tested; Linux desktop validation is deferred until a Linux test environment is available. See the validation scope and remaining follow-up items below.

## Platforms

- **Windows:** Native Rust + [egui](https://github.com/emilk/egui) desktop application. No WebView2, Tauri, Electron, Chromium, or embedded browser runtime.
- **Android:** Kotlin + Jetpack Compose.
- **iOS:** SwiftUI native app.
- **Shared engine:** Rust workspace with versioned protocol types, device registry, pairing consent state machine, local-network discovery, encrypted-session primitives, and trust persistence.

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
- `.github/workflows/` — CI and tag-based release automation.

## Milestone 1: authenticated local connectivity

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

See [docs/roadmap.md](docs/roadmap.md), [docs/protocol.md](docs/protocol.md), [docs/security-model.md](docs/security-model.md), [CONTRIBUTING.md](CONTRIBUTING.md), and [SECURITY.md](SECURITY.md).

## CI/CD

All compilation, tests, packaging, and releases run in GitHub Actions. No local build is required.

Every push to `main` and every pull request validates Rust and builds:
1. Rust formatting, Clippy, and unit tests.
2. Native Windows desktop executable using egui.
3. Android debug APK.
4. iOS Simulator app.

Push a version tag such as `v0.2.0` to build release assets and publish a GitHub Release automatically.

**Windows driver note:** Bridge is evaluating the vendor-signed Parsec Virtual Display Driver 0.45.1 as a third-party dependency. Windows CI verifies its x64 driver catalog signature and pinned archive hash (`9792e4121d85ed3e4c40c2d8ba36ec8657e13227a8357d719d923e801238ccdd`). Bridge will not build or sign its own IddCx driver. Driver bundling remains gated on confirming redistribution permissions. Bridge's Windows desktop/MSI/service binaries may still need their own application code-signing strategy, which is separate from display-driver signing. See [Second Screen architecture](docs/second-screen-architecture.md).

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

The Windows + Android Milestone 1 target is validated on physical devices. iOS and Linux remain build/platform validation follow-ups, not claims of completed physical-device interoperability. A successful CI build or loopback test alone is not proof of real-device interoperability.

## License

MIT. See [LICENSE](LICENSE).
