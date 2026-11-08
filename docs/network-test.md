# Bridge Milestone 1 device validation

This checklist covers the authenticated local-network pairing implementation. CI builds and loopback tests are necessary, but only physical-device runs can close the interoperability gates.

## QR invitation flow

1. Connect the devices to the same private Wi-Fi/LAN and open Bridge.
2. On the device that will accept the connection, open **Devices** and choose **Show my QR**. Windows uses **Generate pairing QR** in its secure pairing dialog.
3. On the initiating Android or iOS device, choose **Scan QR** and scan the invitation.
4. Confirm that the scanner accepts only a `bridge://pair/` URI. An unrelated QR code must not start a connection.
5. Confirm the scanned invitation is consumed once, and the expected Noise fingerprint is supplied to the connection attempt.
6. Verify that both devices show the same six-digit SAS. Approve only after comparing both screens; reject a mismatch.
7. Confirm the peer appears in the trusted-device list and reconnects using its pinned identity.
8. Scan the same invitation again and confirm replay rejection. Wait more than two minutes and confirm expiry rejection.
9. Revoke the trusted device and verify the active session closes and the peer must pair again.

QR invitations are endpoint hints, not authentication tokens. Never bypass Noise XX, fingerprint pinning, SAS comparison, or explicit approval.

## Required physical-device matrix

- [ ] Windows ↔ Android: QR bootstrap, direct discovery, SAS approval, trusted reconnect, revocation.
- [ ] Windows ↔ iOS: QR bootstrap, direct discovery, SAS approval, trusted reconnect, revocation.
- [ ] Android ↔ iOS: QR bootstrap, direct discovery, SAS approval, trusted reconnect, revocation.

Run each pairing direction where practical (display on each platform, scan from Android/iOS) and record OS version, device model, Bridge build SHA, network type, and result. Windows QR is designed to be scanned by a mobile client; Windows camera scanning is not part of the current UX.

## Network and lifecycle matrix

- [ ] Same Wi-Fi access point, IPv4.
- [ ] Different access points on the same LAN.
- [ ] Different router vendors / guest Wi-Fi.
- [ ] Client/AP isolation enabled (expected failure with a clear explanation).
- [ ] IPv6-only and dual-stack networks (record unsupported cases explicitly).
- [ ] Windows firewall allow and deny behavior.
- [ ] Wi-Fi disconnect/reconnect during discovery and during handshake.
- [ ] Android/iOS app background and foreground transitions.
- [ ] Windows sleep/resume and mobile device sleep/wake.
- [ ] Change Wi-Fi network after pairing; verify stale endpoint hints do not override identity pinning.
- [ ] Repeated malformed, oversized, expired, and replayed QR payloads.
- [ ] Camera permission denied/revoked and scanner unavailable states.
- [ ] Listener shutdown, app termination, and restart while handshakes are pending.

## Evidence required to close the milestone

For every physical test, attach sanitized logs and record the tested commit SHA. Do not include private keys, invitation URIs, QR screenshots, or other bearer-like bootstrap data in public logs or issue reports.

A successful CI run does not complete these checks. Milestone 1 also requires an independent security review of discovery, invitation validation/replay handling, Noise identity pinning, trust persistence, and revocation.
