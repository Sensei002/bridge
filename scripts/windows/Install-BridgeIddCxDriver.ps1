[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$PackageDirectory,

    [Parameter(Mandatory = $true)]
    [ValidatePattern('^(?i)[0-9a-f]{40}$')]
    [string]$ExpectedSignerThumbprint,

    [switch]$TrustTestCertificate
)

$ErrorActionPreference = "Stop"

function Assert-Administrator {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw "Run this script from an elevated PowerShell window (Run as administrator)."
    }
}

Assert-Administrator
$package = (Resolve-Path -LiteralPath $PackageDirectory -ErrorAction Stop).Path
$inf = Join-Path $package "IddSampleDriver.inf"
$catalog = Join-Path $package "IddSampleDriver.cat"
$driverDll = Join-Path $package "IddSampleDriver.dll"
$certificateFile = Join-Path $package "Bridge-Driver-CodeSigning.cer"

foreach ($path in @($inf, $catalog, $driverDll, $certificateFile)) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Driver package is incomplete; required file is missing: $path"
    }
}

$expected = ($ExpectedSignerThumbprint -replace '\s', '').ToUpperInvariant()
$certificate = [Security.Cryptography.X509Certificates.X509Certificate2]::new($certificateFile)
$actual = $certificate.Thumbprint.ToUpperInvariant()
if ($actual -ne $expected) {
    throw "Package certificate thumbprint does not match the expected Bridge signer."
}

$codeSigningOid = "1.3.6.1.5.5.7.3.3"
$eku = $certificate.Extensions |
    Where-Object { $_ -is [Security.Cryptography.X509Certificates.X509EnhancedKeyUsageExtension] } |
    Select-Object -First 1
if (-not $eku -or -not ($eku.EnhancedKeyUsages | Where-Object { $_.Value -eq $codeSigningOid })) {
    throw "The package certificate does not contain the Code Signing EKU."
}

$manifest = Join-Path $package "SHA256SUMS.txt"
if (Test-Path -LiteralPath $manifest -PathType Leaf) {
    $entries = Get-Content -LiteralPath $manifest
    if ($entries.Count -lt 3) { throw "SHA256SUMS.txt is incomplete." }
    foreach ($entry in $entries) {
        if ($entry -notmatch '^([0-9a-fA-F]{64})\s+\*?(.+)$') {
            throw "Malformed SHA-256 manifest entry."
        }
        $expectedHash = $Matches[1].ToUpperInvariant()
        $fileName = $Matches[2].Trim()
        $filePath = Join-Path $package $fileName
        if (-not (Test-Path -LiteralPath $filePath -PathType Leaf)) {
            throw "Manifest references a missing package file: $fileName"
        }
        $actualHash = (Get-FileHash -LiteralPath $filePath -Algorithm SHA256).Hash
        if ($actualHash -ne $expectedHash) {
            throw "SHA-256 mismatch for package file: $fileName"
        }
    }
}

if ($TrustTestCertificate) {
    Write-Warning "Trusting the self-signed Bridge test certificate in LocalMachine Root and TrustedPublisher. Only do this for a certificate obtained from a trusted Bridge build."
    Import-Certificate -FilePath $certificateFile -CertStoreLocation "Cert:\LocalMachine\Root" | Out-Null
    Import-Certificate -FilePath $certificateFile -CertStoreLocation "Cert:\LocalMachine\TrustedPublisher" | Out-Null
}

$signature = Get-AuthenticodeSignature -FilePath $catalog
if ($signature.Status -ne "Valid") {
    throw "Driver catalog signature is not trusted (status: $($signature.Status)). Verify the package and certificate trust; do not bypass signature checks."
}
if (-not $signature.SignerCertificate -or $signature.SignerCertificate.Thumbprint.ToUpperInvariant() -ne $expected) {
    throw "Driver catalog signer does not match the expected Bridge certificate."
}

# PnPUtil validates the signed catalog/INF relationship and stages the package.
$pnputil = Join-Path $env:SystemRoot "System32\pnputil.exe"
$output = & $pnputil /add-driver $inf /install 2>&1
$exitCode = $LASTEXITCODE
$output | ForEach-Object { Write-Host $_ }
if ($exitCode -ne 0) {
    throw "PnPUtil failed to stage/install the Bridge IddCx driver (exit code $exitCode)."
}

Write-Host "Bridge IddCx driver package was accepted by Windows."
Write-Host "Signer: $actual"
Write-Host "A reboot may be required before the virtual monitor is available."
Write-Warning "This script does not enable test-signing mode, disable Secure Boot, or remove existing driver packages."
