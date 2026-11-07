# Contributing to Bridge

Keep changes focused, documented, and testable.

## Principles
- Keep protocol changes versioned and backward-compatible where practical.
- Never weaken pairing, authentication, or permission checks for convenience.
- Do not commit credentials, signing keys, private device data, or production secrets.
- Platform features must respect operating-system permission models.

## CI checks
The GitHub Actions pipeline builds the Rust core, Windows desktop client, Android API 37 APK, and iOS Simulator app on pushes and pull requests.

## Pull requests
Describe the user-visible change, affected platforms, test commands/results, and protocol or security implications.
