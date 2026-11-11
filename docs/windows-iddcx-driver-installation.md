# Installing the Bridge IddCx test driver

The signed x64 driver artifact is produced by the **Windows Indirect Display Driver** workflow on trusted `development` or `stable` pushes. Pull-request builds are unsigned and are not installable by this script.

The package must contain:

- `IddSampleDriver.inf`
- `IddSampleDriver.cat`
- `IddSampleDriver.dll`
- `Bridge-Driver-CodeSigning.cer`
- `SHA256SUMS.txt`

From an elevated PowerShell window, first inspect the signing certificate thumbprint from the trusted workflow artifact. Then install with the exact configured Bridge signer thumbprint:

```powershell
.\scripts\windows\Install-BridgeIddCxDriver.ps1 `
  -PackageDirectory .\bridge-iddcx-package `
  -ExpectedSignerThumbprint YOUR_40_CHARACTER_CERTIFICATE_THUMBPRINT
```

The script checks the package file hashes when a manifest is present, verifies the certificate's Code Signing EKU and expected thumbprint, requires a valid catalog signature from that exact signer, and delegates INF/catalog staging to Windows PnPUtil.

## Self-signed development certificate

The existing CI certificate is self-signed. Windows must trust it before accepting the driver. Only if the artifact and certificate were obtained from a trusted Bridge build, explicitly opt in to trusting it on this machine:

```powershell
.\scripts\windows\Install-BridgeIddCxDriver.ps1 `
  -PackageDirectory .\bridge-iddcx-package `
  -ExpectedSignerThumbprint YOUR_40_CHARACTER_CERTIFICATE_THUMBPRINT `
  -TrustTestCertificate
```

This imports the certificate into the Local Machine Root and Trusted Publishers stores. It is a development/test trust arrangement, not Microsoft Hardware Dev Center attestation or production driver distribution.

The installer does not enable Windows test-signing mode, disable Secure Boot, uninstall older packages, or remove the certificate from trust stores. It never bypasses catalog signature verification. A reboot may be required. Disconnect the Bridge virtual display and use Windows recovery options before experimenting with display-driver changes on a machine you rely on.

To inspect staged driver packages, use:

```powershell
pnputil /enum-drivers
```

Driver package removal is intentionally not automated here: deleting the wrong published INF can disrupt other display devices.
