# Bridge pairing

## Automatic local-network discovery

Bridge clients advertise and browse for peers using mDNS/DNS-SD (Windows), Android NSD, and iOS Bonjour. Devices on the same local network appear automatically in the **Devices** section. Discovery metadata is only a candidate hint; it is not trusted identity.

## One-tap pairing

1. Select a nearby device and tap **Connect**.
2. The clients establish an encrypted Noise XX session and exchange authenticated device descriptors.
3. The receiving device shows a popup with the requesting device name and platform. The user can **Accept** or **Reject**.
4. The decision is sent inside the encrypted session. The initiating device saves trust only after it receives an authenticated acceptance response.
5. Previously trusted devices reconnect automatically using their pinned Noise fingerprint. Revocation removes trust and closes the active session.

First-time approval is trust-on-first-use. Because the user no longer compares a SAS or scans a pinned invitation, this is less resistant to an active first-connection man-in-the-middle than out-of-band verification. The receiving user should accept only a device they recognize. The TCP ping/pong endpoint remains a connectivity diagnostic only; it is not an authenticated session.
