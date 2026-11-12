# Windows Display Service setup from the Bridge MSI

The Bridge Windows MSI remains a per-user desktop installation. It also carries the
signed Display Service executable, its setup scripts, and the SHA-256 publisher pin
for the matching Bridge desktop publisher in:

`%LOCALAPPDATA%\\Programs\\Bridge\\DisplayServiceSetup`

The MSI does **not** silently install or start a privileged Windows service. Service
installation is a separate administrator-approved action.

## Install the service

1. Install Bridge using the signed MSI.
2. Open an elevated PowerShell window (Run as administrator).
3. Run:

```powershell
$setup = Join-Path $env:LOCALAPPDATA 'Programs\\Bridge\\DisplayServiceSetup'
& (Join-Path $setup 'Install-BridgeDisplayService.ps1') `
  -ServiceExecutable (Join-Path $setup 'bridge-display-service.exe')
```

The installer validates the Authenticode signatures of both the desktop executable and
service executable, verifies that both use the exact publisher SHA-256 pin bundled in
the MSI, then copies the service and pin into the protected
`%ProgramFiles%\\Bridge\\DisplayService` directory. The service reads the pin only from
that administrator-protected directory. It does not depend on a mutable environment
variable or a driver-signing certificate.

Unsigned development services are not accepted for authenticated Second Screen. The
legacy `-AllowUnsignedDevelopmentBuild` switch is retained only to return a clear
error; it no longer bypasses publisher verification.

## Remove the service

Before uninstalling Bridge, open elevated PowerShell and run:

```powershell
$setup = Join-Path $env:LOCALAPPDATA 'Programs\\Bridge\\DisplayServiceSetup'
& (Join-Path $setup 'Uninstall-BridgeDisplayService.ps1')
```

The uninstaller removes the Windows service registration. It intentionally leaves the
IddCx driver package installed; driver removal is a separate operation and should only
be performed after confirming no Bridge virtual monitor is active.

The desktop MSI does not automatically remove the service because service removal
requires elevation and an explicit administrator decision.
