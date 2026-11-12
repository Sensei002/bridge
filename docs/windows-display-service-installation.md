# Installing the Bridge Windows display components

## Upstream Virtual Display Driver

Bridge uses the upstream VirtualDrivers Virtual Display Driver 25.7.23. It does not build, sign, or install a Bridge-owned display driver.

The MSI places two setup scripts in the Bridge installation's `DisplayServiceSetup` directory:

- `Install-BridgeVirtualDisplayDriver.ps1`
- `Uninstall-BridgeVirtualDisplayDriver.ps1`

Open an elevated PowerShell window in that directory and run the install script:

```powershell
.\Install-BridgeVirtualDisplayDriver.ps1
```

The script downloads the official driver-only release directly from the upstream GitHub release. It does not redistribute or bundle the driver archive in Bridge's MSI. Before calling Windows PnP installation, it checks the pinned archive SHA-256, requires the expected valid SignPath Foundation catalog signer and thumbprint, verifies the driver DLL is AMD64, and requires Windows' own PnP installer to accept the package. It uses the exact 25.7.23 package validated in Bridge CI.

The script supports PowerShell `-WhatIf` and requires administrator approval. It does not enable the virtual monitor; Bridge requests a display only during an authenticated Second Screen session. A successful `pnputil` command means the driver package was accepted into the Windows Driver Store, not that Windows has enumerated a monitor.

To remove the upstream driver package:

```powershell
.\Uninstall-BridgeVirtualDisplayDriver.ps1
```

The uninstaller first sends the upstream local-only `SETDISPLAYCOUNT 0` request, then removes matching MttVDD packages with `pnputil`. The upstream pipe has no acknowledgement, so this request is not proof that the monitor has disappeared. Windows may require a restart to finish package removal. Do not remove the driver while a Bridge Second Screen session is active.

## Bridge Display Service

The separate Bridge Display Service installer is documented in [Installing the Bridge Display Service](windows-display-service-installation.md). Its Authenticode verification remains enabled by default; unsigned development builds require the explicit development-only switch.

## Current limitation

These scripts are a verified driver-package setup foundation, not yet a one-click transactional installer. The Windows MSI still installs per-user and does not silently elevate, register the privileged service, or roll back both service and driver as one transaction. The full elevated install/upgrade/uninstall experience remains a Milestone 2 task.
