# Contributing to Bridge

Keep changes focused, documented, and testable.

## Principles
- Keep protocol changes versioned and backward-compatible where practical.
- Never weaken pairing, authentication, or permission checks for convenience.
- Do not commit credentials, signing keys, private device data, or production secrets.
- Platform features must respect operating-system permission models.

## CI checks
The GitHub Actions pipeline builds and tests the Rust workspace, native Windows desktop client, and Android APK on pushes and pull requests. Linux support is planned; iOS is not a supported target.

## Pull requests
Describe the user-visible change, affected platforms, test commands/results, and protocol or security implications.
