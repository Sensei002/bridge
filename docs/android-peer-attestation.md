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

## Signed challenge transport

The Android app now handles a display challenge only when the session is associated with an explicitly trusted Windows peer. It reloads the Keystore-wrapped Ed25519 seed for each request, signs the 32-byte nonce together with the request's UUID session ID and the Android Noise identity fingerprint, and clears the temporary seed buffer after signing. The signed proof is returned as a bounded control message inside the existing encrypted Noise session.

The desktop binds the response to the outstanding request ID, its live session UUID, the exact challenge nonce, and the authenticated remote Noise fingerprint. It also enforces the fixed hexadecimal encoding lengths. The desktop does **not** treat this structural check as cryptographic verification.

## Remaining implementation boundaries

This step does not yet:
- Independently verify the Ed25519 signature in the Windows Display Service against the public key pinned during pairing.
- Relay the complete signed proof through the publisher-verified Windows named-pipe client.
- Bind the Windows service's one-time challenge registry to the session UUID and attestation public key.
- Issue or renew a display authorization lease.

The legacy unsigned `CHALLENGE_RESPONSE` submission has been removed from the desktop path; the challenge is allowed to expire rather than accepting an echo as proof. Lease issuance and driver CONNECT/DISCONNECT remain disabled until the Windows service verifies the signed proof and the complete process-bound authorization chain is reviewed.
