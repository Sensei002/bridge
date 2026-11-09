# Bridge update and installation channels

Bridge uses two application update channels:

- **Development** follows the `development` integration branch and publishes prereleases for testing.
- **Stable** is reserved for a future stable branch and will publish production releases.

The first delivery slice packages the native Windows desktop executable as a per-user MSI. The installer places Bridge under the current user's LocalAppData Programs directory and creates a Start Menu shortcut. It does not require administrator privileges for normal installation.

Each development MSI release includes a JSON update manifest with its version, platform, download URL, commit, and SHA-256 checksum. The manifest is the contract that the in-app updater will consume. SHA-256 detects incomplete or corrupted downloads; a future signed-manifest verification step must be enabled before broad public distribution.

The Android updater must use a persistent release-signing key. GitHub-hosted runners generate different default debug keystores, so debug APKs from separate runs cannot reliably update one another. Configure a permanent Android upload/release keystore in GitHub Actions secrets before enabling Android in-place updates. Users who installed earlier CI debug APKs may need a one-time uninstall/reinstall when moving to the persistent signing identity.

The in-app update clients, branch selector UI, signed-manifest verification, and stable channel are subsequent implementation steps; publishing an MSI alone does not enable automatic updates yet.
