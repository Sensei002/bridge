# Secure pairing status

QR pairing is currently disabled in the native clients. The previous QR exchange
was an unauthenticated prototype: it accepted a short code over a plain TCP
connection and did not bind the remote device to a cryptographic identity. It
must not be used to establish trust.

The local-network discovery layer is now active:
- Windows advertises a Bridge DNS-SD service.
- Android browses with Android NSD.
- iOS browses with Bonjour.
- Every discovered result is an unverified candidate only. Names, addresses,
  TXT records, and advertised fingerprints are attacker-controlled until
  authenticated by the Noise handshake.

The secure pairing flow still required before QR pairing can be re-enabled:

1. Exchange a versioned, short-lived QR payload containing only the candidate
   endpoint and a random pairing nonce. The QR payload is not an authentication
   credential.
2. Complete the three-message Noise XX handshake using each device's
   OS-protected static identity key.
3. Derive the same six-digit short authentication string from the authenticated
   Noise static keys and display it on both devices.
4. Require the user to compare both displays and explicitly approve the exact
   peer fingerprint. A mismatch or timeout aborts the attempt.
5. Persist the approved public fingerprint in the local trust store. Future
   sessions must pin that fingerprint; revocation must persist immediately.

Until the native handshake and approval screens are connected end to end, the
clients must continue to show pairing as unavailable. A successful TCP ping,
mDNS result, or matching device name is never proof of identity.
