# Bridge update system

Bridge ships two update channels on Windows and Android:

- **Development** follows the `development` branch and publishes prereleases for testing.
- **Stable** follows the `stable` branch when that branch is introduced. Until its first release, selecting Stable correctly reports that no Stable release is available.

## Release contract

The channel release workflow builds a Windows x64 MSI and, when Android release signing is configured, a signed Android APK. It publishes a `bridge-update.json` asset with this shape:

```json
{
  "schema_version": 1,
  "channel": "development",
  "version": "0.2.123",
  "commit": "<commit-sha>",
  "windows": {
    "version": "0.2.123",
    "asset": "Bridge-0.2.123-x64.msi",
    "sha256": "<sha256>",
    "download_url": "https://github.com/Sensei002/bridge/releases/download/dev-123/Bridge-0.2.123-x64.msi"
  },
  "android": {
    "version_name": "0.2.123",
    "version_code": 123,
    "asset": "Bridge-android.apk",
    "sha256": "<sha256>",
    "download_url": "https://github.com/Sensei002/bridge/releases/download/dev-123/Bridge-android.apk"
  }
}
```

The Android object is omitted until a persistently signed APK is available. The Android package identity is `app.bridge.android`; this must remain unchanged for in-place updates. Clients retrieve release metadata from the GitHub Releases API over HTTPS, require official Bridge release asset URLs, and verify the downloaded package against the manifest SHA-256 before handing it to the platform installer. Android's package installer additionally enforces the APK signing-certificate continuity.

## Android release signing (required for in-place updates)

A permanent signing identity is required. GitHub-hosted debug signing keys are not stable across builds and cannot update one another. Create a release keystore locally and keep a secure offline backup. Never commit the keystore or passwords to the repository.

Example keystore creation (RSA 2048-bit, matching the currently configured release key):

```bash
keytool -genkeypair -v -keystore bridge-release.jks -alias bridge \\
  -keyalg RSA -keysize 2048 -validity 10000
```

Add these repository Actions secrets under **Settings → Secrets and variables → Actions**:

- `ANDROID_KEYSTORE_BASE64` — the keystore file encoded as one Base64 string.
- `ANDROID_KEYSTORE_PASSWORD` — keystore password.
- `ANDROID_KEY_ALIAS` — alias used when creating the key.
- `ANDROID_KEY_PASSWORD` — private-key password.

On Linux, encode the keystore with `base64 -w 0 bridge-release.jks`. On PowerShell, use:

```powershell
[Convert]::ToBase64String([IO.File]::ReadAllBytes("bridge-release.jks"))
```

Do not rotate or lose this key: Android will reject future APKs signed with a different certificate as updates. The workflow intentionally publishes an Android update asset only when all four secrets are present. Without them, CI still produces a debug APK artifact for testing, but Android in-app updates remain unavailable.

**Signing migration warning:** users moving from earlier CI debug APKs to the permanent release key may not be able to install the signed APK over the existing app. Do not uninstall an existing installation until its signing certificate and a safe app-data/pairing migration path have been checked. A one-time reinstall may otherwise be required.

## Client behavior

### Windows

- The selected channel is saved in the user's Bridge settings.
- Bridge checks for updates on startup and offers a manual check in **Settings → Software updates**.
- A newer MSI is downloaded to the user's temporary directory and verified before it is offered.
- Choosing **Install and restart** launches Windows Installer in passive mode and exits Bridge so files can be replaced.

### Android

- The selected channel is saved in app preferences.
- **Settings → Software updates → Check for updates** checks the selected channel.
- A newer APK is downloaded to the app cache and SHA-256 verified.
- Bridge opens Android's system package installer. The user must confirm installation; ordinary apps cannot silently install updates.

## Security notes

- Release discovery and manifest retrieval use GitHub's official HTTPS API and release-asset URLs.
- The clients reject non-HTTPS endpoints and unexpected release asset locations.
- SHA-256 detects corrupted or substituted downloads relative to the retrieved release manifest.
- Android's installer enforces package signing identity; the permanent keystore must be protected.
- Windows MSI Authenticode signing is not configured yet. Windows may show an unknown-publisher warning until a trusted code-signing certificate is added.
