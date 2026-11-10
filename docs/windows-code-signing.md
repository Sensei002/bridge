# Bridge Windows Signing and Trust Plan

## Decision

Bridge will use a **Bridge-owned, self-managed code-signing certificate**. The plan must remain zero-cost: no paid code-signing certificate, paid signing service, SignPath dependency, or paid timestamping service.

The certificate is intended to sign Bridge's Windows driver package and, where useful for the service trust design, Bridge-owned Windows executables and installer. It is not a Microsoft-issued production driver signature. A self-signed certificate is not automatically trusted by Windows or by every PC.

## Certificate ownership and private-key protection

- Create one long-lived Bridge code-signing certificate with a clearly identified subject, such as CN=Bridge Project.
- Keep the private key out of Git, release assets, source archives, logs, and ordinary build artifacts.
- Store the password-protected PFX as a GitHub Actions secret named BRIDGE_DRIVER_SIGNING_PFX_BASE64; store its password as BRIDGE_DRIVER_SIGNING_PFX_PASSWORD.
- Store the public certificate thumbprint as a non-secret repository variable named BRIDGE_DRIVER_CERT_THUMBPRINT.
- Restrict who can change workflows and access signing secrets. Do not expose secrets to pull-request workflows from forks or untrusted branches.
- Use the same certificate for subsequent releases so installed PCs can continue to recognize the Bridge publisher. Plan certificate renewal and an overlap period before expiry.
- Never generate a new signing identity on every CI run. That would break trust continuity and make updates require repeated certificate approval.

The certificate must be generated once in a controlled environment, exported as a password-protected PFX, and then added to GitHub Secrets by the repository owner. The repository includes scripts/windows/New-BridgeDriverSigningCertificate.ps1 to create the key pair and export the PFX/CER outside the repository. GitHub Actions cannot configure repository secrets through the normal workflow token; this setup is a one-time owner action. The script removes the temporary certificate-store entry after export; keep the PFX and password backed up securely.

## GitHub Actions signing pipeline

The Windows driver workflow:

1. Builds the IddCx/UMDF driver and produces its INF, DLL, and catalog.
2. Keeps pull-request builds unsigned; PR jobs do not receive signing secrets.
3. Signs only trusted pushes to development/stable, or manual runs explicitly dispatched from those branches.
4. Imports the PFX into the ephemeral Windows runner and confirms its thumbprint matches BRIDGE_DRIVER_CERT_THUMBPRINT.
5. Signs the driver package catalog using SHA-256.
6. Verifies the catalog signature, the INF's membership in the catalog, and the signer thumbprint.
7. Exports only the public .cer certificate and creates a SHA-256 manifest for the package.
8. Uploads a clearly labelled self-signed-test artifact. The PFX and password are never included.
9. Removes temporary PFX material and imported certificate-store entries in a finally block.

A trusted signing run must fail if signing configuration is missing or the thumbprint does not match. It must never silently label an unsigned package as signed. No timestamp service is configured in this zero-cost initial pipeline; certificate expiry and renewal therefore remain explicit lifecycle concerns.

The signing workflow change is currently under CI validation. The first signed artifact will not be produced by pull-request CI; it requires a trusted-branch push or manual dispatch from development/stable.

## Installer and first-run trust flow

The intended Windows installation experience is:

1. Download the Bridge installer and verify its published SHA-256 hash.
2. Run the installer; Windows may initially show an Unknown Publisher warning because the installer is not yet trusted.
3. Before installing the driver, show the Bridge certificate subject and thumbprint and ask an administrator to approve trusting that exact certificate.
4. Install the public certificate into the appropriate Windows certificate stores only after explicit approval.
5. Verify the signed driver catalog, expected Bridge signer thumbprint, and package file hashes.
6. Install the driver package through the supported Windows PnP driver installation path.
7. Re-verify signatures and hashes before any driver update. Reject mismatched, unsigned, or tampered packages.

The public certificate may be bundled with the installer, but bundling alone does not make it trusted. The installer must not silently add a root certificate or lower Windows security settings.

## Windows compatibility boundary

A self-signed certificate provides a Bridge identity; it does **not** turn a test signature into a Microsoft-trusted production driver signature. Windows policy varies by version, Secure Boot state, Memory Integrity/HVCI, and driver package type. Test-signed packages can be rejected by normal Windows driver policy even when the catalog signature is cryptographically valid and the certificate is trusted locally.

Therefore:

- Do not claim universal installation on every Windows PC until tested on clean Windows 10/11 machines with Secure Boot and Memory Integrity enabled.
- Do not disable Secure Boot, Memory Integrity, or BitLocker as an automatic installer step.
- Do not enable Test Mode automatically.
- If Windows refuses the package under its normal security policy, stop and explain the exact limitation. Do not bypass the policy.
- Use a disposable/recoverable test PC for initial installation testing. Do not install the current unsigned or self-signed-test driver on a daily-use PC.
- Investigate Microsoft's current Windows Driver Policy and WHCP/Hardware Dev Center requirements before planning public distribution. A self-signed test catalog is not evidence that the package qualifies for production distribution.

For broad public distribution, investigate a no-cost Microsoft-supported route for this exact UMDF package before deciding that a paid certificate is necessary. The project does not authorize spending money as a workaround.

## Windows app and service signing

The desktop app, display service, and MSI may remain unsigned initially. If the service authorization design requires publisher verification, either:

- sign the relevant Bridge-owned executable with the same Bridge certificate and validate its pinned thumbprint; or
- replace Authenticode publisher verification with a separately reviewed, robust process/session authorization mechanism.

Do not treat PID, process path, file name, or a caller-supplied hash as proof of identity. Do not enable CONNECT/DISCONNECT until the service authorization gates are implemented and tested.

## Current status

- Windows desktop/MSI/service: unsigned.
- Bridge-owned driver-signing certificate: generated by the repository owner; GitHub secrets and thumbprint variable configured by the owner.
- Driver CI: PR builds remain unsigned; trusted development/stable branch runs are configured to produce a self-signed-test catalog artifact.
- First signed artifact: pending a trusted-branch signing run and CI verification.
- Installer trust prompt, signature/hash verification at installation time, and driver installation: not implemented.
- Display-service CONNECT/DISCONNECT: fail-closed and disabled.
- Public Windows distribution: not approved; clean-machine compatibility and current Microsoft driver-policy requirements remain to be validated.

## References

- [Microsoft: Test-signing driver packages](https://learn.microsoft.com/en-us/windows-hardware/drivers/install/test-signing-driver-packages)
- [Microsoft: Test-signing a driver package's catalog file](https://learn.microsoft.com/en-us/windows-hardware/drivers/install/test-signing-a-driver-package-s-catalog-file)
- [Microsoft: SignTool](https://learn.microsoft.com/en-us/windows-hardware/drivers/devtest/signtool)
- [Microsoft Support: The Windows Driver Policy](https://support.microsoft.com/en-us/windows/hardware/drivers/the-windows-driver-policy)
- [Microsoft: Installing test-signed driver packages](https://learn.microsoft.com/en-us/windows-hardware/drivers/install/installing-test-signed-driver-packages)
- [Microsoft: TESTSIGNING boot configuration](https://learn.microsoft.com/en-us/windows-hardware/drivers/install/the-testsigning-boot-configuration-option)
