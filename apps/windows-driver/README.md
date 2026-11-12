# Bridge Windows virtual display integration

## Driver decision

Bridge will use an existing signed x64 VDD package instead of maintaining a Bridge-specific IddCx/UMDF driver.

### Candidate under verification: Parsec VDD 0.45.1

- Integration project: https://github.com/nomi-san/parsec-vdd
- Upstream release: https://github.com/nomi-san/parsec-vdd/releases/tag/v0.45.1
- Package verified by CI: `ParsecVDisplay-v0.45-portable.zip`
- The project documents Windows 10/11 64-bit support. CI extracted the x64 driver package and verified its catalog signature as `Microsoft Windows Hardware Compatibility Publisher`.
- Pinned archive SHA-256: `9792e4121d85ed3e4c40c2d8ba36ec8657e13227a8357d719d923e801238ccdd`.
- The ParsecDisplay companion app is explicitly unsigned; Bridge will not bundle or execute it.
- Parsec VDD requires a keepalive ping roughly every 100 ms (never more than 200 ms apart); if pings stop for about one second, the driver removes the virtual display. Bridge's Windows service must implement that watchdog. A reference Rust client is available at https://github.com/rohitsangwan01/parsec-vdd-rust and must be reviewed before reuse.

The VirtualDrivers VDD candidate was rejected for x64: its 25.7.23 bundle had ARM64 and x86 only, and its older x64 ZIP had a catalog reported as NotSigned by Windows. Bridge will not install the self-signed certificate supplied in that package.

Bridge must use the selected vendor-supplied driver package unchanged. Do not build, modify, catalog-sign, or certificate-sign a Bridge-owned display driver. Do not install a Bridge-owned root certificate or ask users to enable test signing / disable Secure Boot. Do not build, modify, catalog-sign, or certificate-sign a Bridge-owned display driver. Do not install a Bridge-owned root certificate or ask users to enable test signing / disable Secure Boot.

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
