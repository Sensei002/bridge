# Bridge Security Model

Bridge is local-first and permission-first. Discovery is never proof of identity, and pairing never silently grants access to every capability.

## Implemented security foundations

- Device descriptors are version-validated and exchanged only inside an authenticated encrypted session.
- The shared Rust network layer uses Noise XX with Curve25519, ChaChaPoly, SHA-256 fingerprints, bounded length-prefixed TCP frames, socket timeouts, and replay-resistant transport counters.
- Windows, Android, and iOS persist per-installation static identity keys in Windows Credential Manager, Android Keystore-wrapped storage, and iOS Keychain respectively.
- Windows advertises DNS-SD; Android publishes/browses through NSD; iOS publishes/browses through Bonjour. Discovery metadata remains untrusted.
- All three native clients can initiate or accept the shared Rust Noise XX handshake and exchange authenticated device descriptors. A new incoming peer appears as a receiver-side approval popup. Accept/reject is sent back inside the encrypted session; the initiating client does not save trust until it receives acceptance.
- Reconnection to an existing trusted device pins its stored fingerprint. A changed fingerprint is rejected.
- Trusted-device records and revocation persist on each platform: Windows stores public fingerprints in an app-local trust registry; Android encrypts the registry with an Android Keystore AES-GCM key; iOS stores it in Keychain with device-only accessibility. Revocation closes active sessions.
- The Rust mobile FFI has loopback integration tests covering pairing and encrypted message exchange. CI builds the native Windows app, Android APK, iOS Simulator app, and shared Rust workspace.

## Milestone 1 validation status

The Windows + Android target has passed user-reported physical-device testing for discovery, bidirectional pairing, receiver approval, trusted reconnection, app restart/reconnection, Wi-Fi disconnect/reconnect, and firewall/network permissions. iOS Simulator CI builds pass, but iOS physical-device interoperability is unverified. Linux desktop testing is deferred until a Linux environment is available.

The following remain recommended follow-up work before a broad public release: broader router/AP-isolation and IPv6 coverage, lifecycle/resource-limit stress testing, and an independent security review of discovery parsing, first-use trust approval, Noise identity pinning, trust persistence, and revocation.

Private keys must never leave their device. Discovery names, addresses, TXT records, and advertised fingerprints are attacker-controlled. Noise XX encrypts and authenticates the session key, but without an out-of-band comparison it does not independently prove that a first-time peer is the physical device the user intended. First-time pairing therefore uses trust-on-first-use: the receiver must recognize the device shown in the approval popup before accepting. This is simpler than SAS entry but provides less protection against an active first-connection man-in-the-middle than comparing a SAS or verifying a fingerprint through a separate trusted channel. Users should accept only devices they can independently identify.

## Legacy prototype pairing disabled

The former BRIDGE_PAIR/1 exchange compared a short code over an unauthenticated TCP connection. It did not run Noise or pin a peer identity. It remains disabled. The TCP ping/pong endpoint is a connectivity diagnostic only and is not an authenticated session.


## File-transfer security (Milestone 2)

The shared `bridge-transfer` crate validates bounded offers and chunks, enforces a 2 GiB initial file-size ceiling, checks a SHA-256 digest before publication, rejects path-like filenames, and publishes received files without overwriting existing destinations. Incoming data is staged in a temporary file that is removed if the transfer is dropped before successful verification.

These primitives are not a substitute for transport authentication: clients must send control messages and chunks only through the already-authenticated Noise session, and must obtain explicit receiver approval before accepting file data. UI and session integration, physical-device testing, and independent review remain outstanding while Milestone 2 is in progress.
