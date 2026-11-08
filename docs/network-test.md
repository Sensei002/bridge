# Bridge Milestone 1 device validation

This document records the physical-device validation scope for Milestone 1. Windows + Android are the currently available and validated target platforms. iOS physical-device and Linux desktop testing are deferred until suitable test hardware/environments are available.

## Windows ↔ Android: completed

- [x] Automatic discovery on the same LAN.
- [x] Windows → Android: Connect, receiver approval, and successful pairing.
- [x] Android → Windows: Connect, receiver approval, and successful pairing.
- [x] Previously trusted devices reconnect without another approval prompt.
- [x] App restart and trusted reconnection on both platforms.
- [x] Wi-Fi disconnect/reconnect; user reports behaviour is working.
- [x] Windows firewall and Android network-permission behaviour; user reports both are working.
- [x] Local revocation closes the local active session. Trust is device-local: revoking on one endpoint does not automatically revoke the other endpoint's stored trust. A peer that still trusts the device may permit a quick trusted reconnection; this is the intended current behaviour.

## Deferred platform coverage

- [ ] Windows ↔ iOS physical-device interoperability (requires iPhone and Mac test environment).
- [ ] Android ↔ iOS physical-device interoperability (requires iPhone and Mac test environment).
- [ ] Linux desktop client installation and physical-device interoperability (deferred until Linux is installed).

## Additional hardening coverage

These are useful follow-up tests, but are not blockers for declaring the Windows + Android Milestone 1 target complete:

- [ ] Different router vendors / guest Wi-Fi.
- [ ] Client/AP isolation enabled (expected failure with a clear explanation).
- [ ] IPv6-only and dual-stack networks (record unsupported cases explicitly).
- [ ] Wi-Fi changes during an active handshake.
- [ ] Android background/foreground and Windows sleep/resume stress testing.
- [ ] Listener shutdown and app termination while handshakes are pending.
- [ ] Explicit rejection-path regression test on physical devices.

## Validation notes

The user physically tested Windows ↔ Android on the current Bridge build and confirmed bidirectional pairing, successful approval, trusted reconnection, app restart/reconnection, working Wi-Fi reconnect, and working firewall/network permissions. CI run #180 passed on commit `c77ecc7b54c7975f52c897604ba6d69ee66079d3`.

Do not include private keys or other sensitive session data in public logs or issue reports. This checklist records user-reported device results; it is not an independent security audit.
