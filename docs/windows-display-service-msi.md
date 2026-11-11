# Windows Display Service setup from the Bridge MSI

The Bridge Windows MSI remains a per-user desktop installation. It now also carries the
Display Service executable and its setup scripts in:

`%LOCALAPPDATA%\Programs\Bridge\DisplayServiceSetup`

The MSI does **not** silently install or start a privileged Windows service. Service
installation is a separate administrator-approved action.

## Install the service

1. Install Bridge using the MSI.
2. Open an elevated PowerShell window (Run as administrator).
3. Review the bundled PowerShell installer before running it. The script copies the
   executable into `%ProgramFiles%\Bridge\DisplayService`, applies protected ACLs,
   verifies the executable signature by default, and registers the service as LocalSystem.
4. Run:

```powershell
$setup = Join-Path $env:LOCALAPPDATA 'Programs\Bridge\DisplayServiceSetup'
& (Join-Path $setup 'Install-BridgeDisplayService.ps1') `
  -ServiceExecutable (Join-Path $setup 'bridge-display-service.exe')
```

The installer rejects an executable without a valid Authenticode signature. Do not
disable this check for binaries from untrusted sources.

### Trusted development builds only

A CI pull-request MSI can contain an unsigned development executable. For a development
build that you personally built or verified from trusted Bridge source, the installer
has an explicit opt-in:

```powershell
& (Join-Path $setup 'Install-BridgeDisplayService.ps1') `
  -ServiceExecutable (Join-Path $setup 'bridge-display-service.exe') `
  -AllowUnsignedDevelopmentBuild
```

This bypass is only for controlled local development. It is not appropriate for normal
installation or distribution. A production installation requires the signed service
binary and the matching Bridge desktop publisher configuration.

## Remove the service

Before uninstalling Bridge, open elevated PowerShell and run:

```powershell
$setup = Join-Path $env:LOCALAPPDATA 'Programs\Bridge\DisplayServiceSetup'
& (Join-Path $setup 'Uninstall-BridgeDisplayService.ps1')
```

The uninstaller removes the Windows service registration. It intentionally leaves the
IddCx driver package installed; driver removal is a separate operation and should only
be performed after confirming no Bridge virtual monitor is active.

The desktop MSI does not automatically remove the service because service removal
requires elevation and an explicit administrator decision.
