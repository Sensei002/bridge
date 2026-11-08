<p align="center">
  <img src="assets/Bridge%20%E2%80%94%20Connect,%20Share,%20Beyond.png" alt="Bridge — Connect, Share, Beyond" width="760" />
</p>

<h1 align="center">Bridge</h1>
<p align="center"><strong>CONNECT · SHARE · BEYOND</strong></p>
<p align="center">One ecosystem. Every device.</p>

Bridge is an open-source, local-first device interoperability project for Windows, Android, and iOS. The goal is to make nearby devices work together through one secure, permission-based ecosystem.

> **Status: Milestone 1 foundation implemented; not yet complete.** Native Windows, Android, and iOS clients automatically discover Bridge peers on the local network. A user selects **Connect**, the receiving device displays an approval popup, and the initiator waits for that decision over the encrypted Noise session. New peers are trusted only after the receiving device accepts; previously trusted devices reconnect automatically. Physical-device interoperability testing, lifecycle hardening, and independent security review remain outstanding.

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

Still required before Milestone 1 is complete:

- Physical-device end-to-end pairing tests for Windows ↔ Android, Windows ↔ iOS, and Android ↔ iOS, including trusted reconnection and revocation.
- Physical network/lifecycle validation across router configurations, isolation, sleep/resume, and network changes.
- Final connection lifecycle, resource-limit, reconnection, and platform network-permission hardening.
- Independent security review of the complete pairing flow.

See [docs/roadmap.md](docs/roadmap.md), [docs/protocol.md](docs/protocol.md), [docs/security-model.md](docs/security-model.md), [CONTRIBUTING.md](CONTRIBUTING.md), and [SECURITY.md](SECURITY.md).

## CI/CD

All compilation, tests, packaging, and releases run in GitHub Actions. No local build is required.

Every push to `main` and every pull request validates Rust and builds:
1. Rust formatting, Clippy, and unit tests.
2. Native Windows desktop executable using egui.
3. Android debug APK.
4. iOS Simulator app.

Push a version tag such as `v0.2.0` to build release assets and publish a GitHub Release automatically.

**Signing note:** Windows output is unsigned, Android release artifacts are unsigned, and iOS archive is unsigned. Store distribution requires signing certificates/provisioning profiles and repository secrets; none are committed to this repository.

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

The automatic discovery and one-tap pairing flow is implemented in the native clients. Physical-device interoperability, lifecycle/network validation, and independent security review remain release gates. Do not treat a successful CI build or loopback test as proof of real-device interoperability.

## License

MIT. See [LICENSE](LICENSE).
