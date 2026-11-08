# Secure pairing status

## Direct local-network pairing

Direct pairing from the discovered-device list is implemented on Windows, Android, and iOS. It uses the shared Rust Noise XX handshake, authenticated descriptor exchange, six-digit SAS comparison, explicit approval, fingerprint pinning for known devices, and persistent trust/revocation.

Discovery is only a bootstrap mechanism. Service names, addresses, TXT records, and advertised fingerprints are untrusted until the handshake authenticates the peer.

## QR invitation

QR pairing is enabled through the versioned `bridge://pair/` invitation format. The previous BRIDGE_PAIR/1 QR exchange was an unauthenticated prototype and must never be restored.

A shared Rust QR invitation codec is now implemented and tested. It emits a versioned `bridge://pair/` payload with a literal IP endpoint, port, device ID, display name, 64-character Noise fingerprint, random nonce, and a two-minute expiry. Decoding rejects malformed/oversized payloads, hostnames, invalid ports, invalid fingerprints, unsupported versions, expired invitations, and timestamps too far in the future. Native QR display is integrated into Windows egui, Android Compose, and iOS SwiftUI. Android uses Google Code Scanner and iOS uses VisionKit to scan; both pass the raw URI to the Rust one-time nonce consumer, then connect using the invitation fingerprint as the expected Noise identity. Windows displays a QR invitation for mobile devices to scan. The QR payload remains only an endpoint hint. Both screens must still display the same SAS and both users must explicitly approve before a trust record is written. The scanner and QR UI still require physical-device validation across all three platform pairings.

The TCP ping/pong endpoint remains a connectivity diagnostic only; it is not an authenticated session.
