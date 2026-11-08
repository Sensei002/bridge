# Secure pairing status

## Direct local-network pairing

Direct pairing from the discovered-device list is implemented on Windows, Android, and iOS. It uses the shared Rust Noise XX handshake, authenticated descriptor exchange, six-digit SAS comparison, explicit approval, fingerprint pinning for known devices, and persistent trust/revocation.

Discovery is only a bootstrap mechanism. Service names, addresses, TXT records, and advertised fingerprints are untrusted until the handshake authenticates the peer.

## QR invitation

QR pairing remains disabled. The previous BRIDGE_PAIR/1 QR exchange was an unauthenticated prototype and must never be restored.

A replacement QR flow remains a Milestone 1 task. Its payload must be a short-lived connection invitation only—not a trust credential. The scanned endpoint and fingerprint must still be checked by Noise XX, both screens must display the same SAS, and both users must explicitly approve before a trust record is written.

The TCP ping/pong endpoint remains a connectivity diagnostic only; it is not an authenticated session.
