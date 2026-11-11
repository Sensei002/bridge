# Signed Bridge Windows desktop and service binaries

The `Windows Trusted Binaries` workflow builds both `bridge-desktop.exe` and `bridge-display-service.exe`. Pull-request builds are unsigned. Only trusted pushes to `development` or `stable` (or a manual run on those branches) can access the protected `bridge-driver-signing` environment and produce the signed test artifact.

The signing job verifies the configured certificate thumbprint, requires the Code Signing EKU, signs both executables with SHA-256, and verifies each Authenticode signature. It also exports the public certificate and writes the certificate's SHA-256 hash to `BRIDGE_DESKTOP_SIGNER_SHA256.txt`.

## Development machine provisioning

The privileged display service deliberately rejects authorization requests unless the desktop executable has a valid Authenticode signature from the exact configured publisher. On a dedicated test PC, obtain the signed artifact and its certificate from the same trusted workflow run.

1. Verify the artifact came from the expected Bridge repository and trusted branch.
2. As an administrator, provision the public certificate into the machine's Root and TrustedPublisher stores only after reviewing its subject and thumbprint.
3. Set the machine-level `BRIDGE_DESKTOP_SIGNER_SHA256` environment variable to the exact 64-character value in `BRIDGE_DESKTOP_SIGNER_SHA256.txt`.
4. Install the signed display service using the elevated `Install-BridgeDisplayService.ps1` script and run the signed desktop executable from the same artifact.

The environment variable is machine-level because the service runs as LocalSystem. Restart the service after changing it. Do not copy the private PFX into the test PC or commit signing material to the repository.

This is a self-signed development identity, not a Microsoft-trusted production publisher or Microsoft-attested driver signature. Do not use it to imply public Windows distribution readiness. The workflow does not install the driver, change Secure Boot, enable test-signing mode, or alter Windows trust stores on a user's PC.
