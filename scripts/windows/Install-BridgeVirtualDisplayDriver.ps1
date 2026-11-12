[CmdletBinding(SupportsShouldProcess = $true)]
param([string]$DownloadDirectory = (Join-Path $env:TEMP "Bridge-VDD-25.7.23"))
$ErrorActionPreference = "Stop"
$releaseUrl = "https://github.com/VirtualDrivers/Virtual-Display-Driver/releases/download/25.7.23/VirtualDisplayDriver-x86.Driver.Only.zip"
$expectedArchiveSha256 = "e24210692b442b39af763536330ce78b423f19342b7a7792c26de3944e418b3a"
$expectedSignerThumbprint = "3CF8CF26D8BA266C3A483AB7D26D4A818E317D76"
$expectedSignerName = "SignPath Foundation"
$nefconUrl = "https://github.com/nefarius/nefcon/releases/download/v1.14.0/nefcon_v1.14.0.zip"
$expectedNefconSha256 = "a15557da24a9efca203158de3b43b0eaf982db231f0194031f1ed428bc13e669"

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
function Get-BridgeVddDevices {
    @(Get-PnpDevice -PresentOnly:$false -ErrorAction SilentlyContinue |
        Where-Object { $_.InstanceId -match '(?i)^ROOT\\MTTVDD\\' })
}

Assert-Administrator
if (-not [Environment]::Is64BitOperatingSystem) { throw "Bridge Second Screen currently requires 64-bit Windows." }
if (-not $PSCmdlet.ShouldProcess("VirtualDrivers VDD 25.7.23", "Download, verify, and create the signed AMD64 root-enumerated display device")) { return }

New-Item -ItemType Directory -Path $DownloadDirectory -Force | Out-Null
$archive = Join-Path $DownloadDirectory "VirtualDisplayDriver-x86.Driver.Only.zip"
$extract = Join-Path $DownloadDirectory "package"
$nefconArchive = Join-Path $DownloadDirectory "nefcon_v1.14.0.zip"
$nefconExtract = Join-Path $DownloadDirectory "nefcon"
if (Test-Path -LiteralPath $extract) { Remove-Item -LiteralPath $extract -Recurse -Force }
if (Test-Path -LiteralPath $nefconExtract) { Remove-Item -LiteralPath $nefconExtract -Recurse -Force }

# The INF must be installed against a ROOT\MttVDD device instance. pnputil
# alone only stages the package; it does not create this root-enumerated device.
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

$existingPackages = @(Get-InstalledBridgeVddPackages)
if ($existingPackages.Count -gt 0) {
    Write-Host "Verified upstream VDD package is already in the Windows Driver Store; staging is not repeated."
    Write-Host "Published package(s): $($existingPackages.Driver -join ', ')"
} else {
    $pnputil = Join-Path $env:SystemRoot "System32\pnputil.exe"
    & $pnputil /add-driver $infs[0].FullName
    $exitCode = $LASTEXITCODE
    if ($exitCode -notin @(0, 3010)) { throw "Windows rejected the signed VDD package (pnputil exit code $exitCode)." }
    if ($exitCode -eq 3010) { Write-Warning "Windows requested a restart to complete driver staging." }
}

# NefCon is the open-source DevCon-compatible root-device installer used by
# the upstream VDD project's own silent-install instructions. Pin its release.
Write-Host "Downloading the pinned NefCon root-device installer..."
Invoke-WebRequest -Uri $nefconUrl -OutFile $nefconArchive -UseBasicParsing
$nefconHash = (Get-FileHash -LiteralPath $nefconArchive -Algorithm SHA256).Hash.ToLowerInvariant()
if ($nefconHash -ne $expectedNefconSha256) { throw "NefCon archive SHA-256 mismatch. Expected $expectedNefconSha256; received $nefconHash. Nothing was executed." }
Expand-Archive -LiteralPath $nefconArchive -DestinationPath $nefconExtract -Force
$nefcon = Join-Path $nefconExtract "x64\nefconw.exe"
if (-not (Test-Path -LiteralPath $nefcon -PathType Leaf)) { throw "The pinned NefCon archive does not contain x64\nefconw.exe." }

$devices = @(Get-BridgeVddDevices)
if ($devices.Count -eq 0) {
    Write-Host "Creating the missing ROOT\MttVDD device instance..."
    & $nefcon install $infs[0].FullName "Root\MttVDD"
    $nefconExit = $LASTEXITCODE
    if ($nefconExit -ne 0) { throw "NefCon could not create ROOT\MttVDD (exit code $nefconExit)." }
} else {
    Write-Host "ROOT\MttVDD device instance already exists; not creating a duplicate."
}

$deviceDeadline = (Get-Date).AddSeconds(30)
do {
    Start-Sleep -Milliseconds 500
    $devices = @(Get-BridgeVddDevices)
} while ($devices.Count -eq 0 -and (Get-Date) -lt $deviceDeadline)
if ($devices.Count -eq 0) {
    throw "The driver package is staged, but Windows did not enumerate ROOT\MttVDD after NefCon installation. Check Device Manager for a driver start error."
}
$devices | Format-Table Status,Class,FriendlyName,InstanceId -AutoSize

$pipeDeadline = (Get-Date).AddSeconds(30)
do {
    Start-Sleep -Milliseconds 500
    $pipeReady = Test-Path -LiteralPath '\\.\pipe\MTTVirtualDisplayPipe'
} while (-not $pipeReady -and (Get-Date) -lt $pipeDeadline)
if (-not $pipeReady) {
    throw "ROOT\MttVDD exists, but the upstream MTTVirtualDisplayPipe did not become available. Check the VDD device status and Microsoft Visual C++ Redistributable."
}

Write-Host "SUCCESS: signed VirtualDrivers VDD is staged, ROOT\MttVDD is enumerated, and its control pipe is available."
Write-Host "The virtual monitor is still activated only during an authenticated Bridge Second Screen session."
