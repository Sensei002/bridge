# Bridge Security Model

Bridge is local-first and permission-first. Discovery is never proof of identity, and pairing must never silently grant access to every capability.

## Current implementation

- Device descriptors are version-validated.
- Pairing requests expire after at most two minutes.
- Pairing requires explicit local approval before a device enters the in-memory trusted registry.
- The Rust security crate implements a Noise XX handshake using Curve25519, ChaChaPoly, and SHA-256, plus authenticated encrypted transport sessions and tamper-detection tests.
- The handshake exposes the remote static public key so the pairing layer can compare it with a key or fingerprint the user explicitly approved.

The security crate is a cryptographic foundation, not yet a complete production network stack. It does not yet bind the platform identity adapters, socket handshake, fingerprint pinning, and persistent trust registry into one end-to-end pairing flow. Windows Credential Manager, Android Keystore-wrapped storage, and iOS Keychain adapters now persist device identity material, but their presence alone does not establish an authenticated peer session. The current in-memory pairing registry must not mark a peer trusted until the integration performs explicit user approval and remote-key verification.

## Required before network pairing ships

1. Per-installation cryptographic identity stored in Windows DPAPI/Credential Manager, Android Keystore, or Apple Keychain/Secure Enclave where available.
2. Transcript-bound QR / short-authentication-string verification integrated with the Noise XX handshake. The trust-store confirmation API now requires the handshake-derived SAS as well as both user-visible codes to match.
3. A network transport with bounded framing, timeouts, replay protection, and connection limits.
4. Persistent trust storage with user-visible revocation.
5. Independent capability grants for clipboard, files, notifications, camera, microphone, and input control.

Private keys must never leave their device. Any relay must be opt-in and must not store user content by default.


## Legacy prototype pairing disabled

The former `BRIDGE_PAIR/1` QR exchange only compared a short code over an unauthenticated TCP connection. It did not run Noise, pin a peer identity, or provide cryptographic authentication. It has been disabled in both native clients and must never be used to establish trust. The TCP ping/pong endpoint remains a connectivity diagnostic only and is not an authenticated session.
