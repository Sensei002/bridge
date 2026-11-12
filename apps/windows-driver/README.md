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
- Uploads an unsigned package for pull-request validation; trusted release builds will sign the catalog with the protected Bridge certificate once the signing secrets are configured.

## Scope and limitations

- The adapter initializes without a monitor. The privileged Bridge Display Service uses the control interface to request monitor arrival and departure.
- The service executable, service installer, authenticated-session binding, and service-side watchdog are not implemented in this milestone.
- The modes are an initial driver-side capability list, not a guarantee that the current Android decoder/network path can sustain them. Runtime capability negotiation must gate production use.
- The Bridge-specific swap-chain loop acquires IddCx Direct3D surfaces, performs bounded BGRA8 staging readback, and publishes the latest frame through the Bridge frame sink. H.264 encoding and authenticated delivery are owned by the Windows service/desktop pipeline, not by the driver.
- The current CI artifact is unsigned and is not ready for installation. The target signing design uses a stable Bridge-owned certificate stored as protected GitHub Actions secrets. The signed catalog and public certificate will be packaged only from trusted release builds. A self-signed certificate does not guarantee acceptance under every Windows security configuration; validate on clean Windows machines without disabling Secure Boot or Memory Integrity. Do not install the current unsigned package on a daily-use PC.

## Upstream source and license

- Repository: https://github.com/microsoft/Windows-driver-samples
- Sample: `video/IndirectDisplay/IddSampleDriver`
- Pinned commit: `d49bb7d72f3651952b2b6e4ad28d14fe2247eb69`
- License: Microsoft Public License (MS-PL), reproduced in [THIRD_PARTY_LICENSE.txt](IddSampleDriver/THIRD_PARTY_LICENSE.txt)

Bridge-specific monitor configuration, INF identity, and project metadata are maintained separately from the upstream sample. Upstream copyright and license notices are retained.
\n\n## Why Bridge keeps its own IddCx implementation\n\nThe VirtualDrivers Virtual Display Driver is a useful reference for monitor configuration, but its current upstream swap-chain loop explicitly leaves frame processing as a TODO and releases acquired surfaces without publishing desktop frames. Its signed binary is therefore not a drop-in replacement for Bridge: Bridge requires a frame source connected to its authenticated Windows service and Android H.264 pipeline. Replacing the Bridge driver with that unmodified binary would create a monitor but would not complete Second Screen. Keep the Bridge-specific IddCx driver and improve its installer/signing path; reuse upstream code only after a concrete, tested integration proves equivalent frame delivery and lifecycle behavior.\n