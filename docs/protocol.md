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


## Secure file transfer (Milestone 2)

File-transfer controls and file chunks are application messages carried only inside an established, authenticated Noise transport session. Discovery metadata must never authorize a transfer.

The initial transfer flow is:

1. Sender computes the file size and SHA-256 digest, then sends a bounded `file_offer` containing a random transfer ID, safe display filename, size, and digest.
2. Receiver validates the offer and asks the user to accept or reject it. No destination file is created before approval.
3. After acceptance, sender streams sequential 48 KiB chunks. Each chunk carries the transfer ID and exact byte offset and is encrypted by the existing Noise transport.
4. Receiver writes only to an app-generated temporary file, rejects wrong IDs, gaps, overlaps, oversized chunks, and bytes beyond the declared size.
5. Receiver verifies the final SHA-256 digest before publishing the file. Existing files are never overwritten; a numbered filename is selected instead.
6. Either side may cancel. Incomplete temporary files are deleted.

The first implementation caps individual files at 2 GiB. Resume/restart support is deliberately deferred until the basic transfer path has passed physical-device testing. A sender must not infer success until it receives an authenticated completion response.
