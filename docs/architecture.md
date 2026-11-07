# Bridge Architecture

Bridge is a local-first device interoperability system for Windows, Android, and iOS.

## Layers
1. Protocol: versioned message envelopes and stable wire types.
2. Core: platform-independent device/session/capability logic.
3. Transport: planned discovery and authenticated encrypted connections.
4. Clients: Tauri 2 on Windows, Kotlin/Compose on Android, SwiftUI on iOS.

Capabilities are negotiated and permissioned per device. Local-network operation is the default. Any future cloud relay must be explicit and opt-in.

## Current status
The initial foundation defines protocol types and an in-memory device registry. Network discovery, pairing, and data transfer are not implemented yet.
