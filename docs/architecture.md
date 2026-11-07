# Bridge Architecture

Bridge uses native clients with a shared Rust domain layer, versioned wire protocol, and a dedicated secure-session crate.

## Clients

- Windows: Rust + egui native desktop UI. No WebView2, Tauri, Electron, Chromium, or embedded browser.
- Android: Kotlin + Jetpack Compose.
- iOS: SwiftUI.

## Shared engine

- `bridge-protocol`: stable, versioned message types and validation.
- `bridge-core`: device registry and explicit-consent pairing lifecycle.
- `bridge-security`: Noise XX over Curve25519, ChaChaPoly, and SHA-256. It exposes handshake messages, remote static-key inspection, and authenticated encrypted transport sessions.
- Platform adapters will expose native APIs to the shared engine through a narrow FFI boundary where appropriate.

## Connectivity and trust

Discovery results are untrusted hints, never proof of identity. The Noise XX handshake authenticates the possession of each peer's static private key and establishes an encrypted transport. The pairing layer must compare the remote static key or its fingerprint with the key the user explicitly approved before marking the device trusted.

The security crate does not persist private keys, perform discovery, open sockets, or automatically trust first contact. Those responsibilities remain with platform secure storage, transport, and the pairing UX. Capabilities must remain disabled until user consent and peer verification both succeed.

## Build model

GitHub Actions owns all compile, test, package, and release jobs. Windows desktop is built directly with Cargo from `apps/desktop/Cargo.toml`; it does not require Node.js or a browser runtime.
