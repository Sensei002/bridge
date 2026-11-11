# Installing the Bridge Display Service

Run an elevated PowerShell window. The installer accepts a service executable produced by a trusted Bridge build:

    .\scripts\windows\Install-BridgeDisplayService.ps1 -ServiceExecutable .\bridge-display-service.exe

The installer requires a valid Authenticode signature by default. For a locally built, unsigned development artifact only, explicitly opt in:

    .\scripts\windows\Install-BridgeDisplayService.ps1 -ServiceExecutable .\bridge-display-service.exe -AllowUnsignedDevelopmentBuild

Before registering the service, the installer copies the executable into:

%ProgramFiles%\\Bridge\\DisplayService\\bridge-display-service.exe

It applies a protected directory ACL using Windows well-known SIDs: LocalSystem and Administrators receive full control; Users receive read/execute only. The service therefore does not run from a user-writable Downloads or AppData location. The installer verifies that the copied executable's SHA-256 hash matches the source, registers the service as LocalSystem with automatic startup, configures recovery after unexpected crashes, and starts it.

The installer stops an existing service before replacing its executable. Re-run it from an elevated PowerShell window to update the service binary.

Uninstall the service registration with:

    .\scripts\windows\Uninstall-BridgeDisplayService.ps1

The uninstaller intentionally does not delete the service executable directory or the IddCx driver package. Remove those only after confirming the virtual display is disconnected and no service process is using the files.
