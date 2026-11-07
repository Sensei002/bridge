# Bridge

**One ecosystem. Every device.**

Bridge is an open-source, local-first device interoperability platform for Windows, Android, and iOS. It aims to make nearby devices work together through one secure, permission-based ecosystem.

> Project status: early foundation. The protocol and application shells are being established; discovery, pairing, transport, and device capabilities are not yet implemented.

## Product goals

- Pair devices securely and make trust/revocation visible to users.
- Share text, links, files, and images across paired devices.
- Support device capabilities through an extensible, versioned protocol.
- Keep local-network communication local; make any future relay explicit and opt-in.
- Provide native-feeling clients on Windows, Android, and iOS.
- Keep the protocol and core libraries open for third-party clients.

## Platform strategy

| Platform | Client | Implementation |
| --- | --- | --- |
| Windows | Bridge Desktop | Tauri 2, Rust core, React + TypeScript UI |
| Android | Bridge for Android | Kotlin, Jetpack Compose |
| iOS | Bridge for iOS | Swift, SwiftUI |

A shared Rust core is intended to own protocol types, identity, pairing, permissions, discovery, and transport. Native platform layers will expose only the capabilities each operating system permits.

## Repository layout

- `crates/bridge-protocol` — versioned wire types and protocol compatibility.
- `crates/bridge-core` — platform-independent device/session/capability logic.
- `apps/desktop` — Windows desktop client (Tauri 2).
- `apps/android` — Android client (Kotlin / Compose).
- `apps/ios` — iOS client (Swift / SwiftUI).
- `docs/` — architecture, protocol, security, and roadmap.

## Development status

This repository is being bootstrapped. The first milestone is a buildable multi-platform skeleton and documented protocol boundary. Subsequent milestones add, in order:

1. Device identity, pairing, and revocation.
2. Local discovery and authenticated encrypted sessions.
3. Text/link clipboard sync with explicit per-device permissions.
4. File transfer with progress, cancellation, and size limits.
5. Platform-specific capabilities (notifications, media controls, camera/microphone) where supported by OS APIs and user consent.

## Security principles

- Pairing must require user approval on both devices.
- Every device has a revocable identity.
- Capabilities are opt-in and independently permissioned.
- Network messages are versioned, validated, and bounded.
- No silent cloud upload or background relay.
- Secrets are never committed to the repository.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Security issues should be reported privately; see [SECURITY.md](SECURITY.md).

## License

MIT. See [LICENSE](LICENSE).
