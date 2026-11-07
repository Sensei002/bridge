# Bridge Architecture

Bridge uses native clients with a shared Rust domain layer and versioned wire protocol.

## Clients

- Windows: Rust + egui native desktop UI. No WebView2, Tauri, Electron, Chromium, or embedded browser.
- Android: Kotlin + Jetpack Compose.
- iOS: SwiftUI.

## Shared engine

- `bridge-protocol`: stable, versioned message types and validation.
- `bridge-core`: device registry, pairing lifecycle, permissions, and capability model.
- Platform adapters expose native APIs to the shared engine through a narrow FFI boundary where appropriate.

## Connectivity

Local discovery and authenticated encrypted transport are planned. Discovery results are untrusted hints, never proof of identity. Pairing requires explicit consent and cryptographic peer verification before any capability is enabled.

## Build model

GitHub Actions owns all compile, test, package, and release jobs. Windows desktop is built directly with Cargo from `apps/desktop/Cargo.toml`; it does not require Node.js or a browser runtime.
