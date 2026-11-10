# Android peer attestation

## Purpose

Bridge's Windows Display Service must not treat a desktop client's claim that an Android challenge was echoed as proof that Android participated. The Android device must sign a fresh service challenge with a per-device Ed25519 identity key, and the service must verify it against the public key pinned during explicit pairing.

## Signed transcript

The shared `bridge-security::peer_attestation` module defines a domain-separated transcript:

1. Fixed domain tag `BRIDGE_ANDROID_PEER_ATTESTATION_V1\0`.
2. 32-byte, one-time service nonce.
3. 16-byte UUID identifying the live desktop/Android Noise session.
4. 64-character hexadecimal Android Noise identity fingerprint, normalized to lowercase ASCII.

Ed25519 signs the exact concatenated bytes. The verifier checks the expected nonce, session UUID, and fingerprint before verifying the signature with the public key supplied from the trusted pairing record. A public key carried in an untrusted attestation message must never be used as its own trust anchor.

## Implementation boundaries

This PR adds the shared Rust signing and strict verification primitives plus tests for wrong keys, modified signatures, stale nonces, cross-session replay, and fingerprint mismatch.

It does not yet:
- Generate or persist the Android Ed25519 identity in Android Keystore-backed storage.
- Exchange and pin the Ed25519 public key as part of explicit pairing.
- Add the attestation request/response to the encrypted Noise control channel.
- Send the signed attestation through the verified Windows named-pipe client.
- Connect verification to the Windows service challenge registry or issue a lease.

Those are follow-on steps. Until the key is securely pinned during pairing and the complete round trip is verified by the service, lease issuance and driver CONNECT/DISCONNECT must remain disabled.
