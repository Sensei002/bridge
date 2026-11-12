# Bridge Windows installation (NSIS)

Bridge for Windows is distributed as a signed, 64-bit NSIS installer. It requests
administrator approval once through the standard Windows UAC prompt and installs
machine-wide under:

`C:\Program Files\Bridge`

## What setup installs

- Bridge Desktop, running normally as the signed-in user (not elevated)
- The upstream, Authenticode-verified Virtual Display Driver
- Bridge Display Service, registered as an automatic LocalSystem service
- The publisher SHA-256 pin in the protected service directory
- Start Menu and Desktop shortcuts

The installer verifies the desktop and service Authenticode signatures against the
publisher pin before registering the privileged service. It does not require a
separate PowerShell installation step or a reviewer approval.

## Install

1. Download and run `Bridge-<version>-Setup-x64.exe`.
2. Approve the Windows UAC prompt.
3. Allow setup to finish. It downloads and verifies the pinned upstream VDD package,
   installs the driver, then installs and starts Bridge Display Service.
4. Open Bridge and connect the paired Android device. Use the Second Screen Connect
   control; Windows should enumerate the virtual monitor. Choose **Extend** in
   Settings → System → Display if Windows has not already selected it.

The desktop application itself runs without administrator privileges. Only setup and
the background display service require elevation.

## Updates

Bridge's in-app updater downloads the official release installer, verifies its SHA-256
checksum, and launches it. NSIS requests UAC for the machine-wide update. Close the
app when prompted by setup; the installer also stops an older Bridge process before
replacing the desktop executable.

## Uninstall

Use **Settings → Apps → Installed apps → Bridge → Uninstall**, or run
`C:\Program Files\Bridge\Uninstall.exe` as administrator.

Uninstall stops and removes Bridge Display Service and deletes Bridge application
files and shortcuts. The upstream VDD driver package is intentionally left installed
to avoid removing a display device while Windows may still be using it. It can be
removed separately with the bundled
`DisplayServiceSetup\Uninstall-BridgeVirtualDisplayDriver.ps1` script after
disconnecting Second Screen.

## Troubleshooting

If setup reports a VDD or Display Service installation error, do not bypass signature
verification. Copy the final error from the NSIS setup details. A Windows restart may
be required if the driver installer reports that it requested one.
