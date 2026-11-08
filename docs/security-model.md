# Bridge Security Model

Bridge is local-first and permission-first. Discovery is never proof of identity, and pairing never silently grants access to every capability.

## Implemented security foundations

- Device descriptors are version-validated and exchanged only inside an authenticated encrypted session.
- The shared Rust network layer uses Noise XX with Curve25519, ChaChaPoly, SHA-256 fingerprints, bounded length-prefixed TCP frames, socket timeouts, and replay-resistant transport counters.
- Windows, Android, and iOS persist per-installation static identity keys in Windows Credential Manager, Android Keystore-wrapped storage, and iOS Keychain respectively.
- Windows advertises DNS-SD; Android publishes/browses through NSD; iOS publishes/browses through Bonjour. Discovery metadata remains untrusted.
- All three native clients can initiate or accept the shared Rust Noise XX handshake, exchange authenticated device descriptors, display a six-digit SAS, and require explicit matching-code approval before trusting a new device.
- Reconnection to an existing trusted device pins its stored fingerprint. A changed fingerprint is rejected.
- Trusted-device records and revocation persist on each platform: Windows stores public fingerprints in an app-local trust registry; Android encrypts the registry with an Android Keystore AES-GCM key; iOS stores it in Keychain with device-only accessibility. Revocation closes active sessions.
- The Rust mobile FFI has loopback integration tests covering pairing, SAS agreement, encrypted message exchange, and bounded session handling. Secure QR invitations carry the expected device ID and Noise fingerprint; the authenticated descriptor must match both before the session is registered. CI builds the native Windows app, Android APK, iOS Simulator app, and shared Rust workspace.

## Remaining before Milestone 1 can be called complete

1. Physical-device end-to-end pairing and trusted reconnection/revocation tests for Windows ↔ Android, Windows ↔ iOS, and Android ↔ iOS. CI loopback tests and simulator builds do not replace this.
2. Physical network/lifecycle validation across router vendors, client isolation, IPv4/IPv6, sleep/resume, app backgrounding, and network changes.
3. Final connection lifecycle, resource-limit, shutdown, and platform permission/error-path validation on real devices.
4. Independent security review of discovery parsing, QR expiry/replay, Noise identity pinning, SAS confirmation, trust persistence, and revocation.

Private keys must never leave their device. Discovery names, addresses, TXT records, and advertised fingerprints are attacker-controlled until verified by Noise. A matching SAS and explicit user approval are required for first-time trust.

## Legacy prototype pairing disabled

The former BRIDGE_PAIR/1 exchange compared a short code over an unauthenticated TCP connection. It did not run Noise or pin a peer identity. It remains disabled. The TCP ping/pong endpoint is a connectivity diagnostic only and is not an authenticated session.
