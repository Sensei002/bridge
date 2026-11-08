# Secure pairing status

## Direct local-network pairing

Direct pairing from the discovered-device list is implemented on Windows, Android, and iOS. It uses the shared Rust Noise XX handshake, authenticated descriptor exchange, six-digit SAS comparison, explicit approval, fingerprint pinning for known devices, and persistent trust/revocation.

Discovery is only a bootstrap mechanism. Service names, addresses, TXT records, and advertised fingerprints are untrusted until the handshake authenticates the peer.

## QR invitation

QR pairing remains disabled. The previous BRIDGE_PAIR/1 QR exchange was an unauthenticated prototype and must never be restored.

A shared Rust QR invitation codec is now implemented and tested. It emits a versioned `bridge://pair/` payload with a literal IP endpoint, port, device ID, display name, 64-character Noise fingerprint, random nonce, and a two-minute expiry. Decoding rejects malformed/oversized payloads, hostnames, invalid ports, invalid fingerprints, unsupported versions, expired invitations, and timestamps too far in the future. The QR payload is still only an endpoint hint: native QR rendering/scanning, one-time nonce consumption, and connecting the scanned fingerprint as the expected Noise identity remain to be wired into each client. Both screens must still display the same SAS and both users must explicitly approve before a trust record is written.

The TCP ping/pong endpoint remains a connectivity diagnostic only; it is not an authenticated session.
