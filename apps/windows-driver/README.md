# Bridge Windows virtual display integration

## Driver decision

Bridge will use an existing signed x64 VDD package instead of maintaining a Bridge-specific IddCx/UMDF driver.

### Preferred candidate under inspection: VirtualDrivers VDD 25.7.23

- Upstream: https://github.com/VirtualDrivers/Virtual-Display-Driver
- Package: `VDD.Control.25.7.23.zip`
- Archive SHA-256: `a701f2272e9fcf382849b24f913c6dd07597b3b1116525f2e90182f019609154`
- CI confirms the catalog is signed by SignPath Foundation.
- The archive stores the package under an `x86` folder. CI now reads the PE machine field from `MttVDD.dll` to determine whether the binary is AMD64; the folder label alone is not enough to decide.

### Signed x64 fallback: Parsec VDD 0.45.1

- Integration project: https://github.com/nomi-san/parsec-vdd
- Package: `ParsecVDisplay-v0.45-portable.zip`
- Archive SHA-256: `9792e4121d85ed3e4c40c2d8ba36ec8657e13227a8357d719d923e801238ccdd`
- CI extracts its driver package and verifies the x64 INF/DLL and Microsoft Windows Hardware Compatibility Publisher catalog signature.
- Its companion ParsecDisplay application is unsigned and will not be bundled or executed.
- Parsec VDD requires a keepalive ping about every 100 ms (never more than 200 ms apart); if pings stop for about one second, the virtual display is removed.
- Parsec owns the driver binary; confirm redistribution terms before bundling. Prefer the VirtualDrivers MIT-licensed driver if the PE architecture check confirms AMD64.

Bridge must use the selected vendor-supplied driver package unchanged. Do not build, modify, catalog-sign, or certificate-sign a Bridge-owned display driver. Do not install a Bridge-owned root certificate or ask users to enable test signing / disable Secure Boot. Do not build, modify, catalog-sign, or certificate-sign a Bridge-owned display driver. Do not install a Bridge-owned root certificate or ask users to enable test signing / disable Secure Boot. Do not build, modify, catalog-sign, or certificate-sign a Bridge-owned display driver. Do not install a Bridge-owned root certificate or ask users to enable test signing / disable Secure Boot.

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
