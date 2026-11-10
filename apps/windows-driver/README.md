# Bridge Windows Indirect Display Driver

This directory contains Bridge's development IddCx/UMDF virtual-display driver.

## Current implementation

- Based on Microsoft's pinned IddSampleDriver reference implementation.
- Uses a Bridge-specific root hardware ID and UMDF DeviceGroupId.
- Builds the x64 UMDF driver package in GitHub Actions with a matched Windows SDK/WDK toolchain.
- Exposes one Bridge-branded, EDID-less virtual monitor only on explicit service request.
- Provides a versioned IddCx IOCTL contract for connect, disconnect, and status queries.
- Restricts the driver interface to LocalSystem and local administrators.
- Advertises an initial portrait mode profile: 1080x2340@60, 720x1560@60, and 540x1170@60.
- Uploads the unsigned development package as a CI artifact for inspection.

## Scope and limitations

- The adapter initializes without a monitor. The privileged Bridge Display Service uses the control interface to request monitor arrival and departure.
- The service executable, service installer, authenticated-session binding, and service-side watchdog are not implemented in this milestone.
- The modes are an initial driver-side capability list, not a guarantee that the current Android decoder/network path can sustain them. Runtime capability negotiation must gate production use.
- The Microsoft sample swap-chain loop still releases frames without encoding or transmitting them.
- This package is unsigned and is not ready for installation on a daily-use Windows machine.

Do not install this unsigned package outside a dedicated test environment. Next we will implement the Windows service-side control client and a disposable-machine arrival/removal test before enabling normal installation.

## Upstream source and license

- Repository: https://github.com/microsoft/Windows-driver-samples
- Sample: `video/IndirectDisplay/IddSampleDriver`
- Pinned commit: `d49bb7d72f3651952b2b6e4ad28d14fe2247eb69`
- License: Microsoft Public License (MS-PL), reproduced in [THIRD_PARTY_LICENSE.txt](IddSampleDriver/THIRD_PARTY_LICENSE.txt)

Bridge-specific monitor configuration, INF identity, and project metadata are maintained separately from the upstream sample. Upstream copyright and license notices are retained.
