# Bridge Architecture

Bridge uses native clients with a shared Rust domain and secure transport layer.

## Clients

- Windows: Rust + egui native desktop UI. No WebView2, Tauri, Electron, Chromium, or embedded browser.
- Android: Kotlin + Jetpack Compose with JNI access to the shared Rust mobile library.
- iOS: SwiftUI with a C ABI bridge to the shared Rust mobile library.

## Shared engine

- bridge-protocol: stable, versioned message types and validation.
- bridge-core: device registry, explicit-consent pairing lifecycle, and persistent public trust records.
- bridge-security: Noise XX key agreement and cryptographic primitives.
- bridge-network: DNS-SD discovery, bounded TCP framing, Noise XX handshake, authenticated descriptor exchange, SAS derivation, and fingerprint verification.
- bridge-mobile: C/JNI FFI for native Android/iOS clients to use the exact shared Rust handshake and encrypted session implementation.

## Discovery and pairing

Windows advertises DNS-SD services; Android uses NSD; iOS uses Bonjour. All clients publish and browse local Bridge endpoints. Advertised metadata is untrusted. Selecting a candidate starts a Noise XX handshake; the authenticated peer descriptor and static-key fingerprint are returned only after the handshake completes. Both clients display the same six-digit SAS and require explicit user confirmation before creating trust.

Existing trusted peers are reconnected only when their fingerprint matches the stored pin. Fingerprint changes are rejected. Revoking a peer removes the durable trust record and closes the associated active session.

QR invitation is still disabled. The old unauthenticated prototype must not be re-enabled; a replacement must bootstrap the same Noise XX + SAS approval flow rather than establish trust by QR contents.

## Identity and trust storage

Private identity keys are protected by platform storage:
- Windows Credential Manager.
- Android AES-GCM wrapping with a non-exportable Android Keystore key.
- iOS Keychain with device-only accessibility.

Trusted public fingerprints are persisted separately:
- Windows app-local JSON trust registry (public metadata only).
- Android AES-GCM encrypted SharedPreferences using an Android Keystore key.
- iOS Keychain.

The mobile trust stores migrate legacy records where possible and fail closed on unreadable or unauthenticated data. Windows also blocks approvals if its trust registry cannot be read.

## Build model

GitHub Actions owns all compile, test, package, and release jobs. The iOS workflow builds a platform-aware XCFramework with iOS device ARM64 plus universal ARM64/x86_64 Simulator slices. Windows desktop is built directly with Cargo and does not require a browser runtime.
