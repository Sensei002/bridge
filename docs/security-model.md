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
- The Rust mobile FFI has loopback integration tests covering pairing, SAS agreement, and encrypted message exchange. CI builds the native Windows app, Android APK, iOS Simulator app, and shared Rust workspace.

## Remaining before Milestone 1 can be called complete

1. A new, secure QR invitation/bootstrap flow. The old QR prototype remains disabled because it used unauthenticated TCP. Direct local-network discovery pairing is active.
2. Cross-platform end-to-end testing on physical Windows, Android, and iOS devices and different Wi-Fi/router configurations. CI loopback tests and simulator builds do not replace this.
3. Operational hardening and review on connection limits, lifecycle handling, and platform-specific network permission behavior.

Private keys must never leave their device. Discovery names, addresses, TXT records, and advertised fingerprints are attacker-controlled until verified by Noise. A matching SAS and explicit user approval are required for first-time trust.

## Legacy prototype pairing disabled

The former BRIDGE_PAIR/1 exchange compared a short code over an unauthenticated TCP connection. It did not run Noise or pin a peer identity. It remains disabled. The TCP ping/pong endpoint is a connectivity diagnostic only and is not an authenticated session.
