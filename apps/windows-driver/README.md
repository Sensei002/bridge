# Bridge Windows virtual display integration

## Driver decision

Bridge uses the upstream **VirtualDrivers Virtual Display Driver (VDD)** instead of maintaining a Bridge-specific IddCx/UMDF driver.

- Upstream: https://github.com/VirtualDrivers/Virtual-Display-Driver
- Initial x64 integration candidate: **24.10.27** (older than the latest upstream control bundle because the 25.7.23 archive tested by CI contained no x64 driver)
- Package: `VirtualDisplayDriver-x64.zip`
- Archive SHA-256: pending first Windows CI verification run; it must be pinned before this dependency is accepted
- The upstream project identifies its source repository as MIT-licensed and release 24.10.27 publishes a separate x64 driver ZIP, and its catalog signature will be verified in Windows CI.

Bridge must use the vendor-supplied driver package unchanged. Do not build, modify, catalog-sign, or certificate-sign a Bridge-owned display driver. Do not install a Bridge-owned root certificate or ask users to enable test signing / disable Secure Boot.

## Important integration boundary

VDD is responsible for creating a real Windows virtual monitor. Bridge remains responsible for:

1. Selecting the VDD output in Windows display topology.
2. Capturing that specific output with DXGI Desktop Duplication or Windows Graphics Capture.
3. Feeding captured frames to Bridge's existing H.264 encoder and authenticated Android stream.
4. Tying capture and monitor lifecycle to the trusted Android session.
5. Installing/updating/removing VDD safely from the Bridge installer.

A monitor appearing in Windows is not enough: Second Screen is complete only when the user can select **Extend** and move ordinary windows onto the Android display, with those windows rendered on the phone.

## CI and packaging

`.github/workflows/windows-driver.yml` validates the pinned upstream release archive, archive hash, extracted INF/catalog files, and catalog signature on Windows. It does not compile or sign a Bridge driver.

The upstream release archive is a third-party dependency. Before distributing it inside Bridge, verify the exact x64 package contents and redistribution rights for every bundled component, and preserve all upstream notices. Source-code MIT licensing does not automatically settle every binary/companion-app redistribution question.

## Migration status

The old `IddSampleDriver` source remains temporarily in this repository as a migration reference. It is no longer the selected product driver and must not be used for release builds or installation. Remove the legacy project and its signing secrets/workflows after the upstream capture/control integration is proven.
