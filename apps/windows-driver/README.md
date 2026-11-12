# Bridge Windows virtual display integration

## Driver decision

Bridge will use an existing signed x64 VDD package instead of maintaining a Bridge-specific IddCx/UMDF driver.

## Selected driver: VirtualDrivers VDD 25.7.23

- Upstream: https://github.com/VirtualDrivers/Virtual-Display-Driver
- Package: `VirtualDisplayDriver-x86.Driver.Only.zip`
- Archive SHA-256: pending first CI verification run for the driver-only package
- Driver-only archive contains `MttVDD.dll`, `MttVDD.inf`, and `mttvdd.cat` without the companion VDD Control app.
- The DLL at that path is verified as AMD64 (PE machine `0x8664`); the folder name is misleading.
- Catalog signer: SignPath Foundation; thumbprint `3CF8CF26D8BA266C3A483AB7D26D4A818E317D76`.
- CI verifies the archive hash, AMD64 PE architecture, and catalog signature. CI verifies the INF and DLL using Windows PnP package policy (`signtool verify /pa`). This is a UMDF user-mode driver; kernel-mode `/kp` is not applicable. Physical installation and driver loading still need hardware validation.

Bridge must use the selected vendor-supplied driver package unchanged. Do not build, modify, catalog-sign, or certificate-sign a Bridge-owned display driver. Do not install a Bridge-owned root certificate or ask users to enable test signing / disable Secure Boot. Do not build, modify, catalog-sign, or certificate-sign a Bridge-owned display driver. Do not install a Bridge-owned root certificate or ask users to enable test signing / disable Secure Boot. Do not build, modify, catalog-sign, or certificate-sign a Bridge-owned display driver. Do not install a Bridge-owned root certificate or ask users to enable test signing / disable Secure Boot. Do not build, modify, catalog-sign, or certificate-sign a Bridge-owned display driver. Do not install a Bridge-owned root certificate or ask users to enable test signing / disable Secure Boot.

## Important integration boundary

VDD is responsible for creating a real Windows virtual monitor. Bridge remains responsible for:

1. Selecting the VDD output in Windows display topology.
2. Capturing that specific output with DXGI Desktop Duplication or Windows Graphics Capture.
3. Feeding captured frames to Bridge's existing H.264 encoder and authenticated Android stream.
4. Tying capture and monitor lifecycle to the trusted Android session.
5. Installing/updating/removing VDD safely from the Bridge installer.

A monitor appearing in Windows is not enough: Second Screen is complete only when the user can select **Extend** and move ordinary windows onto the Android display, with those windows rendered on the phone.

## CI and packaging

`.github/workflows/windows-driver.yml` validates the candidate x64 archive, extracted INF/catalog/DLL files, and catalog signature on Windows. The package is not accepted until its archive hash and signer are pinned and its redistribution conditions are reviewed. The workflow does not compile or sign a Bridge driver.

The upstream release archive is a third-party dependency. Before distributing it inside Bridge, verify the exact x64 package contents and redistribution rights for every bundled component, and preserve all upstream notices. Source-code MIT licensing does not automatically settle every binary/companion-app redistribution question.

## Migration status

The old `IddSampleDriver` source remains temporarily in this repository as a migration reference. It is no longer the selected product driver and must not be used for release builds or installation. Remove the legacy project and its signing secrets/workflows after the upstream capture/control integration is proven.
