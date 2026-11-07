# Bridge Security Model

Bridge is local-first and permission-first. Discovery is never proof of identity, and pairing must never silently grant access to every capability.

## Current implementation

- Device descriptors are version-validated.
- Pairing requests expire after at most two minutes.
- Pairing requires explicit local approval before a device enters the in-memory trusted registry.
- The current pairing state machine does not authenticate remote peers, persist trust, or encrypt traffic. Do not treat it as production-secure.

## Required before network pairing ships

1. Per-installation cryptographic identity stored in Windows DPAPI/Credential Manager, Android Keystore, or Apple Keychain/Secure Enclave where available.
2. Authenticated key agreement and transcript-bound QR / short-authentication-string verification.
3. An authenticated encrypted session with replay protection and strict message-size limits.
4. Persistent trust storage with user-visible revocation.
5. Independent capability grants for clipboard, files, notifications, camera, microphone, and input control.

Private keys must never leave their device. Any relay must be opt-in and must not store user content by default.
