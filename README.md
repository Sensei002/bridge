# Bridge

**One ecosystem. Every device.**

Bridge is an open-source, local-first device interoperability project for Windows, Android, and iOS. The goal is to make nearby devices work together through one secure, permission-based ecosystem.

> **Status: foundation build.** Native client shells and the shared Rust protocol/core foundation are in place. Device discovery, secure pairing, clipboard sync, and file transfer are not implemented yet.

## Platforms

- **Windows:** Tauri 2 + Rust + React/TypeScript desktop client.
- **Android:** Kotlin + Jetpack Compose, targeting Android API 37.
- **iOS:** SwiftUI native app, generated from XcodeGen project configuration.
- **Shared core:** Rust workspace with versioned protocol types and an in-memory device registry.

## Repository layout

- `crates/bridge-protocol` — versioned wire types.
- `crates/bridge-core` — platform-independent domain logic.
- `apps/desktop` — Windows desktop app.
- `apps/android` — Android app.
- `apps/ios` — iOS app.
- `docs/` — architecture, protocol, and roadmap.
- `.github/workflows/` — CI and tag-based release automation.

## CI/CD

Every push to `main` and every pull request runs GitHub Actions builds for:
1. Rust formatting, Clippy, and unit tests.
2. Windows desktop installer.
3. Android API 37 debug APK.
4. iOS Simulator app.

Push a version tag such as `v0.1.0` to build release assets and publish a GitHub Release automatically.

**Signing note:** Windows output is currently unsigned, Android release APK is unsigned, and iOS archive is unsigned. Store distribution requires signing certificates/provisioning profiles and repository secrets; none are committed to this repository.

## Security principles

- Pairing must require user approval on both devices.
- Device identities must be revocable.
- Capabilities are independently permissioned.
- Network messages must be versioned, validated, and size-bounded.
- No silent cloud upload or background relay.
- Never commit secrets or signing keys.

## Development roadmap

1. Device identity, secure pairing, and revocation.
2. Local discovery and authenticated encrypted sessions.
3. Clipboard text/link sharing.
4. File transfer with progress, cancellation, and integrity checks.
5. Platform-specific capabilities where operating-system APIs permit.

See [docs/roadmap.md](docs/roadmap.md), [CONTRIBUTING.md](CONTRIBUTING.md), and [SECURITY.md](SECURITY.md).

## License

MIT. See [LICENSE](LICENSE).
