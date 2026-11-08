# Bridge Milestone 1 device validation

This checklist covers the authenticated local-network pairing implementation. CI builds and loopback tests are necessary, but only physical-device runs can close the interoperability gates.

## Automatic discovery and one-tap pairing

1. Connect Windows, Android, and/or iOS devices to the same private Wi-Fi/LAN and open Bridge on each.
2. Open **Devices**. Each client should automatically show the other Bridge devices nearby without scanning or entering anything.
3. On one device, tap **Connect** next to the intended peer.
4. Confirm the receiving device shows a popup with the requesting device name and platform.
5. Tap **Accept** on the receiving device. Confirm the initiating device reports that the request was accepted and both devices show each other as trusted.
6. Tap **Reject** in a separate test. Confirm the initiating device reports rejection and neither side saves a new trust record.
7. Close and reopen Bridge on both devices. Confirm a previously trusted peer reconnects automatically without another approval popup.
8. Revoke the trusted peer and confirm its active session closes; a new connection must require approval again.

## Required physical-device matrix

- [ ] Windows ↔ Android: discovery, Connect, receiver Accept/Reject, trusted reconnect, revocation.
- [ ] Windows ↔ iOS: discovery, Connect, receiver Accept/Reject, trusted reconnect, revocation.
- [ ] Android ↔ iOS: discovery, Connect, receiver Accept/Reject, trusted reconnect, revocation.

Run each connection direction where practical and record OS version, device model, Bridge build SHA, network type, and result. Verify the approval popup appears only on the receiving device and that rejection never creates a trust record.

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
- [ ] Listener shutdown, app termination, and restart while handshakes are pending.

## Evidence required to close the milestone

For every physical test, attach sanitized logs and record the tested commit SHA. Do not include private keys or other sensitive session data in public logs or issue reports.

A successful CI run does not complete these checks. Milestone 1 also requires an independent security review of discovery, first-use approval, Noise identity pinning, trust persistence, and revocation.
