# Bridge Security Model (planned)

Bridge must be designed around explicit trust, least privilege, and local-first operation.

## Device identity
Each installation will create a unique device identity. Private keys must be generated and stored using platform secure storage (Windows credential/DPAPI facilities, Android Keystore, and Apple Keychain/Secure Enclave where available). Private keys must never leave their device.

## Pairing
Pairing must be approved by both devices. A QR code or short authentication string should bind the exchange to the intended nearby peer and reduce accidental or malicious pairing. Users must be able to inspect and revoke trusted devices.

## Sessions and transport
Discovery is not authentication. Every session must authenticate the peer and use modern authenticated encryption. The protocol must reject unsupported versions, malformed messages, oversized payloads, and replayed control messages.

## Capabilities
Clipboard, file access, notifications, camera, microphone, input control, and other capabilities must be independently permissioned. A device pairing must not grant every capability automatically.

## Privacy
Local network is the default transport. Any future internet relay must be opt-in, disclosed, and designed to avoid storing user content. No analytics or cloud upload should be added without clear user-facing disclosure and controls.

## Current implementation status
These are design requirements, not claims about current functionality. The foundation currently contains protocol data types and an in-memory registry only; secure identity, pairing, encryption, and transport remain unimplemented.
