# Windows Code-Signing Plan

## Decision

Bridge will pursue **SignPath Foundation's free open-source code-signing program** for its Windows user-mode software. This is the best first route because the repository is public, uses the MIT license, is actively maintained, has published releases, and documents its functionality.

SignPath Foundation issues and protects the signing certificate in its own name and keeps the private key in its hardware security module. If accepted, Windows will identify **SignPath Foundation** as the verified publisher. This is not a personal certificate for Sensei002 and must not be presented as one.

Application and approval are external steps. A repository maintainer must apply, complete the Foundation's review, configure the SignPath project, and authorize the GitHub trusted build system. No certificate, account, project ID, or signing token is assumed to exist until that process is complete.

References:
- [SignPath Foundation](https://signpath.org/)
- [Eligibility and conditions](https://signpath.org/terms.html)
- [SignPath GitHub trusted build system](https://docs.signpath.io/trusted-build-systems/github)

## What can be prepared in this repository

After acceptance and project configuration, the GitHub release pipeline can request signing for Bridge-owned Windows user-mode outputs, such as:

- `bridge-desktop.exe`
- `bridge-display-service.exe`
- the final Bridge MSI installer

The signing request must originate only from the protected, reviewed release workflow on the approved release branch/tag. Never submit pull-request artifacts, arbitrary workflow-dispatch artifacts, developer-local binaries, or downloaded third-party executables for signing.

The private signing key must never be exported, stored in GitHub Secrets, committed, or placed on a runner. GitHub Actions should submit the build artifact to SignPath's trusted GitHub integration and wait for the signed result. The workflow must verify the returned signature before attaching the artifact to a release.

Signing remains disabled until SignPath has approved Bridge and supplied the required organization/project configuration. Missing configuration must never silently produce a release described as signed.

## Important: the IddCx driver is a separate signing problem

SignPath Foundation's open-source Authenticode program must **not** be assumed to provide a Microsoft-trusted Windows driver package signature.

The Bridge IddCx UMDF driver package needs its own Windows Hardware Dev Center / Partner Center submission path. Microsoft's driver documentation says attestation signing requires a Hardware Dev Center account associated with an EV code-signing certificate; Microsoft signs the submitted driver package. That is a separate identity, certificate, account, cost, and approval path from SignPath's free user-mode application signing.

- [Microsoft driver code-signing requirements](https://learn.microsoft.com/en-us/windows-hardware/drivers/dashboard/code-signing-reqs)
- [Driver signing options and attestation requirements](https://learn.microsoft.com/en-us/windows-hardware/drivers/dashboard/driver-signing-offerings)
- [Partner Center for Windows Hardware](https://learn.microsoft.com/en-us/windows-hardware/drivers/dashboard/)

Until a driver package has been accepted and signed through the required Microsoft process, do not install it on a normal Windows machine. Do not disable Secure Boot or Windows driver-signature enforcement as a workaround.

## SignPath Foundation eligibility checklist

Before applying, confirm the following accurately:

- Repository is public and source code is available under the MIT license.
- Bridge is actively maintained and already has public releases.
- Project functionality and downloads are documented.
- All code submitted for signing is Bridge's own maintained code; do not submit third-party upstream binaries as if Bridge authored them.
- The team responsible for signing is the same team responsible for developing and maintaining the repository.
- The project has a public privacy statement. Bridge's README currently states that it does not silently upload data or use a background relay; maintain a dedicated privacy page before the application if SignPath requests one.
- Team roles, commit/review policy, and signing approvers are accurately declared. Do not invent additional maintainers or approvers.

SignPath may request changes or decline the application. Approval is not guaranteed. Existing unsigned releases remain unsigned; signing applies only to artifacts approved and produced after activation.

## Publisher identity and service authorization

Once the first official signed Bridge desktop executable is available:

1. Verify the actual Authenticode signer certificate from the signed executable.
2. Record the certificate identity and public-key/certificate pinning strategy, including a safe rotation plan.
3. Provision the accepted publisher identity to the Windows service through an administrator-controlled installer/configuration path.
4. Implement and test WinVerifyTrust verification plus exact signer pinning.
5. Keep CONNECT/DISCONNECT disabled if the pin is missing, malformed, expired, revoked, or mismatched.

Do not hard-code an assumed SignPath thumbprint now. Certificate details must come from the actual issued signing certificate, and renewal/rotation must be planned rather than guessed.

## Current status

- Windows desktop/MSI/service builds: unsigned.
- SignPath Foundation application: not yet submitted or approved.
- SignPath organization/project configuration and GitHub integration: not configured.
- Microsoft Hardware Dev Center EV certificate/account: not configured.
- IddCx driver: unsigned; no end-user installation.
- Display-service CONNECT/DISCONNECT: fail-closed and disabled.
