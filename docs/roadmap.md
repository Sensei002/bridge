# Bridge Roadmap

## Foundation
- [x] Rust workspace and shared protocol crate
- [x] Windows, Android, and iOS application shells
- [x] CI workflows for platform build validation
- [x] Device registry
- [x] Pairing consent and expiry state machine

## Next: authenticated local connectivity
- [ ] Persistent per-device cryptographic identity
- [ ] QR-based peer verification and authenticated key agreement
- [ ] mDNS/DNS-SD discovery with spoofing-safe UI
- [ ] Encrypted local sessions and replay protection
- [ ] Persistent trusted-device management and revocation

## Device capabilities
- [ ] Capability permission model
- [ ] Universal clipboard (text first)
- [ ] Local file transfer with explicit destination approval
- [ ] Presence and device status
- [ ] Android notification bridge (subject to platform permissions)
- [ ] iOS-compatible feature set using public platform APIs

## Release quality
- [ ] Signed Windows installer
- [ ] Signed Android release bundle
- [ ] Apple signing and TestFlight distribution
- [ ] End-to-end tests across physical devices
