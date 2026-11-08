# Bridge Architecture

Bridge uses native clients with a shared Rust domain layer, versioned wire protocol, and a dedicated secure-session crate.

## Clients

- Windows: Rust + egui native desktop UI. No WebView2, Tauri, Electron, Chromium, or embedded browser.
- Android: Kotlin + Jetpack Compose.
- iOS: SwiftUI.

## Shared engine

- `bridge-protocol`: stable, versioned message types and validation.
- `bridge-core`: device registry, explicit-consent pairing lifecycle, and persistent public trust records.
- `bridge-security`: Noise XX over Curve25519, ChaChaPoly, and SHA-256.
- `bridge-network`: mDNS/DNS-SD discovery, bounded TCP framing, Noise XX handshake, and short authentication string generation.

## Trust and identity

Discovery metadata is untrusted and never proves identity. Peers must complete Noise XX, compare the same six-digit short authentication string (SAS) on both screens, and explicitly confirm pairing. The core trust store accepts a peer only alongside a `SasConfirmation` created from matching user-confirmed codes. It pins the remote static-key SHA-256 fingerprint and supports persistent storage, last-seen updates, and revocation.

The trust-store file contains public device metadata and fingerprints only. Private static keys must be stored using the operating system's secure storage: Windows DPAPI/Credential Manager, Android Keystore, and iOS Keychain. The shared Rust API does not claim to provide those platform key stores.

## Build model

GitHub Actions owns all compile, test, package, and release jobs. Windows desktop is built directly with Cargo from `apps/desktop/Cargo.toml`; it does not require Node.js or a browser runtime.
