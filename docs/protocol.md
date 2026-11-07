# Bridge Protocol

Wire protocol version 1 is independent of client release versions. Every envelope contains a protocol version, unique message ID, sender timestamp, and typed payload.

## Current message families

- Ping / pong.
- Capability announcements.
- Pairing request and pairing decision messages.

A pairing request carries a request ID, initiator descriptor, nonce, creation time, and expiry. Requests have a maximum two-minute validity window. Receivers must validate protocol versions, device names, timestamps, and payload sizes before acting.

## Pairing lifecycle

1. A device discovers a peer and displays its advertised identity.
2. The initiating device sends a pairing request.
3. The receiving device places it in a pending state and asks the user.
4. Only an explicit approval adds the remote device to the trusted-device registry.
5. Rejected or expired requests are discarded. Users must be able to revoke trusted devices.

The current Rust pairing coordinator implements the consent/expiry state machine. It is not a cryptographic handshake and must not be exposed over an unauthenticated network transport.

## Compatibility and limits

Unknown message types and unsupported protocol versions must be rejected safely. Receivers must enforce message and field size limits. Breaking wire changes require a new protocol version.
