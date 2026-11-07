# Bridge Protocol (draft)

Wire protocol version 1 is independent of client release versions.

Every message envelope contains a protocol version, unique message ID, sender timestamp, and typed payload. Receivers must validate required fields, enforce message-size limits, and reject unknown message types safely. Breaking wire changes require a new protocol version.

Planned message families: discovery, pairing challenge/approval, capability negotiation, clipboard, file transfer, and presence.

This is a draft schema only; no network transport is implemented.
