# Android peer attestation

## Purpose

Bridge's Windows Display Service must not treat a desktop client's claim that an Android challenge was echoed as proof that Android participated. The Android device must sign a fresh service challenge with a per-device Ed25519 identity key, and the service must verify it against the public key pinned during explicit pairing.

## Signed transcript

The shared `bridge-security::peer_attestation` module defines a domain-separated transcript:

1. Fixed domain tag `BRIDGE_ANDROID_PEER_ATTESTATION_V1\\0`.
2. 32-byte, one-time service nonce.
3. 16-byte UUID identifying the live desktop/Android Noise session.
4. 64-character hexadecimal Android Noise identity fingerprint, normalized to lowercase ASCII.

Ed25519 signs the exact concatenated bytes. The verifier checks the expected nonce, session UUID, and fingerprint before verifying the signature with the public key supplied from the trusted pairing record. A public key carried in an untrusted attestation message must never be used as its own trust anchor.

## Android identity storage

The Android app now creates a separate random 32-byte Ed25519 seed during connection-service startup. It is stored in app-private SharedPreferences only as an AES-256-GCM ciphertext. The AES wrapping key is generated in Android Keystore, is non-exportable, and is used with purpose-bound authenticated data so the attestation seed ciphertext cannot be substituted for another local record.

A persistent initialization marker makes missing seed data fail closed instead of silently generating a replacement identity that would no longer match a previously paired public key. The Android manifest disables application backup, avoiding restoration of ciphertext without its device-bound Keystore key.

The Ed25519 seed itself is not a native Android Keystore Ed25519 key: it is decrypted into app memory when the future signing path requests it, then should be cleared after use. Android Keystore protects the wrapping key; hardware-backed protection depends on the device and is not assumed. The service startup validates that the seed can be loaded and immediately clears that temporary byte array.

## Implementation boundaries

The shared Rust signing and strict verification primitives include tests for wrong keys, modified signatures, stale nonces, cross-session replay, and fingerprint mismatch. This implementation adds Android seed generation and Keystore-protected persistence.

It does not yet:
- Derive and exchange the Ed25519 public key as part of explicit pairing.
- Pin that public key to the authenticated Android Noise fingerprint in the trusted pairing record.
- Add the attestation request/response to the encrypted Noise control channel.
- Send the signed attestation through the verified Windows named-pipe client.
- Connect verification to the Windows service challenge registry or issue a lease.

Those are follow-on steps. Until the key is securely pinned during pairing and the complete round trip is verified by the service, lease issuance and driver CONNECT/DISCONNECT must remain disabled.
