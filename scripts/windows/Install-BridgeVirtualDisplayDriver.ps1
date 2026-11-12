[CmdletBinding(SupportsShouldProcess = $true)]
param([string]$DownloadDirectory = (Join-Path $env:TEMP "Bridge-VDD-25.7.23"))
$ErrorActionPreference = "Stop"
$releaseUrl = "https://github.com/VirtualDrivers/Virtual-Display-Driver/releases/download/25.7.23/VirtualDisplayDriver-x86.Driver.Only.zip"
$expectedArchiveSha256 = "e24210692b442b39af763536330ce78b423f19342b7a7792c26de3944e418b3a"
$expectedSignerThumbprint = "3CF8CF26D8BA266C3A483AB7D26D4A818E317D76"
$expectedSignerName = "SignPath Foundation"
function Assert-Administrator {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw "Bridge Virtual Display Driver setup requires administrator approval. Re-run this script elevated."
    }
}
function Get-InstalledBridgeVddPackages {
    Import-Module Dism -ErrorAction Stop
    @(Get-WindowsDriver -Online -All | Where-Object { $_.OriginalFileName -match '(?i)\\MttVDD\.inf$' })
}
Assert-Administrator
if (-not [Environment]::Is64BitOperatingSystem) { throw "Bridge Second Screen currently requires 64-bit Windows." }
if (-not $PSCmdlet.ShouldProcess("VirtualDrivers VDD 25.7.23", "Download, verify, and install signed AMD64 display driver")) { return }

# NSIS can invoke this script more than once across installer retries. If the
# pinned upstream VDD is already in the Driver Store, treat that as success
# instead of reinstalling it and potentially failing on an already-staged INF.
$existingPackages = @(Get-InstalledBridgeVddPackages)
if ($existingPackages.Count -gt 0) {
    Write-Host "VirtualDrivers VDD is already present in the Windows Driver Store; skipping reinstall."
    Write-Host "Published package(s): $($existingPackages.Driver -join ', ')"
    exit 0
}

New-Item -ItemType Directory -Path $DownloadDirectory -Force | Out-Null
$archive = Join-Path $DownloadDirectory "VirtualDisplayDriver-x86.Driver.Only.zip"
$extract = Join-Path $DownloadDirectory "package"
if (Test-Path -LiteralPath $extract) { Remove-Item -LiteralPath $extract -Recurse -Force }
Write-Host "Downloading the official VirtualDrivers 25.7.23 driver-only package..."
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
Invoke-WebRequest -Uri $releaseUrl -OutFile $archive -UseBasicParsing
$actualHash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
if ($actualHash -ne $expectedArchiveSha256) { throw "Upstream VDD archive SHA-256 mismatch. Expected $expectedArchiveSha256; received $actualHash. Nothing was installed." }
Expand-Archive -LiteralPath $archive -DestinationPath $extract -Force
$infs = @(Get-ChildItem -LiteralPath $extract -Recurse -File -Filter "MttVDD.inf")
$dlls = @(Get-ChildItem -LiteralPath $extract -Recurse -File -Filter "MttVDD.dll")
$catalogs = @(Get-ChildItem -LiteralPath $extract -Recurse -File -Filter "mttvdd.cat")
if ($infs.Count -ne 1 -or $dlls.Count -ne 1 -or $catalogs.Count -ne 1) { throw "The pinned VDD archive did not contain exactly one INF, AMD64 DLL, and catalog. Nothing was installed." }
$catalogSignature = Get-AuthenticodeSignature -LiteralPath $catalogs[0].FullName
if ($catalogSignature.Status -ne "Valid" -or $catalogSignature.SignerCertificate.Thumbprint -ne $expectedSignerThumbprint -or $catalogSignature.SignerCertificate.Subject -notmatch [regex]::Escape($expectedSignerName)) {
    throw "The VDD catalog is not validly signed by the pinned SignPath Foundation certificate. Nothing was installed."
}
$bytes = [IO.File]::ReadAllBytes($dlls[0].FullName)
if ($bytes.Length -lt 256 -or [BitConverter]::ToUInt16($bytes, 0) -ne 0x5A4D) { throw "MttVDD.dll is not a valid PE image. Nothing was installed." }
$peOffset = [BitConverter]::ToInt32($bytes, 0x3C)
if ($peOffset -lt 0 -or ($peOffset + 6) -gt $bytes.Length -or [BitConverter]::ToUInt16($bytes, $peOffset + 4) -ne 0x8664) {
    throw "The pinned VDD package does not contain the expected AMD64 driver. Nothing was installed."
}
$pnputil = Join-Path $env:SystemRoot "System32\\pnputil.exe"
& $pnputil /add-driver $infs[0].FullName /install
$exitCode = $LASTEXITCODE
if ($exitCode -notin @(0, 3010)) { throw "Windows rejected the signed VDD package (pnputil exit code $exitCode)." }
$after = @(Get-InstalledBridgeVddPackages)
if ($after.Count -eq 0) { throw "pnputil returned success, but Windows Driver Store does not report the MttVDD package." }
Write-Host "VirtualDrivers VDD 25.7.23 is present in the Windows Driver Store."
Write-Host "Published package(s): $($after.Driver -join ', ')"
if ($exitCode -eq 3010) { Write-Warning "Windows requested a restart to complete driver installation." }
Write-Host "The virtual monitor is not enabled by this setup step. Bridge enables it only during an authenticated Second Screen session."
