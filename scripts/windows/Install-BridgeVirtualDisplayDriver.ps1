[CmdletBinding(SupportsShouldProcess = $true)]
param(
    [string]$DownloadDirectory = (Join-Path $env:TEMP "Bridge-VDD-25.7.23"),
    [string]$PayloadDirectory = ""
)
$ErrorActionPreference = "Stop"

# NSIS can launch 32-bit PowerShell, where ProgramFiles points to Program Files (x86).
# Bridge is installed under the native 64-bit Program Files directory.
if ([string]::IsNullOrWhiteSpace($PayloadDirectory)) {
    $nativeProgramFiles = if ([Environment]::Is64BitOperatingSystem -and $env:ProgramW6432) {
        $env:ProgramW6432
    } else {
        $env:ProgramFiles
    }
    $PayloadDirectory = Join-Path $nativeProgramFiles "Bridge\DisplayServiceSetup\VDD"
}
Write-Host "Bridge VDD activation payload destination: $PayloadDirectory"
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

Assert-Administrator
if (-not [Environment]::Is64BitOperatingSystem) { throw "Bridge Second Screen currently requires 64-bit Windows." }
if (-not $PSCmdlet.ShouldProcess("VirtualDrivers VDD 25.7.23", "Verify and stage the signed driver package without creating a display device")) { return }

New-Item -ItemType Directory -Path $DownloadDirectory -Force | Out-Null
New-Item -ItemType Directory -Path $PayloadDirectory -Force | Out-Null
$archive = Join-Path $DownloadDirectory "VirtualDisplayDriver-x86.Driver.Only.zip"
$extract = Join-Path $DownloadDirectory "package"
$nefconArchive = Join-Path $DownloadDirectory "nefcon_v1.14.0.zip"
$nefconExtract = Join-Path $DownloadDirectory "nefcon"
if (Test-Path -LiteralPath $extract) { Remove-Item -LiteralPath $extract -Recurse -Force }
if (Test-Path -LiteralPath $nefconExtract) { Remove-Item -LiteralPath $nefconExtract -Recurse -Force }

# Stage only the signed package. Do NOT create ROOT\\MttVDD here: doing so
# makes Windows enumerate a monitor during NSIS setup and can move every window.
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
    Write-Host "Verified upstream VDD package is already in the Windows Driver Store."
    Write-Host "Published package(s): $($existingPackages.Driver -join ', ')"
} else {
    $systemDirectory = if ([Environment]::Is64BitOperatingSystem -and -not [Environment]::Is64BitProcess) {
        # A 32-bit PowerShell process is redirected away from System32. Sysnative
        # is the documented alias that reaches the native 64-bit system directory.
        Join-Path $env:windir "Sysnative"
    } else {
        Join-Path $env:windir "System32"
    }
    $pnputil = Join-Path $systemDirectory "pnputil.exe"
    if (-not (Test-Path -LiteralPath $pnputil -PathType Leaf)) {
        throw "Windows pnputil.exe was not found at the native system path: $pnputil"
    }
    & $pnputil /add-driver $infs[0].FullName
    $exitCode = $LASTEXITCODE
    if ($exitCode -notin @(0, 3010)) { throw "Windows rejected the signed VDD package (pnputil exit code $exitCode)." }
    if ($exitCode -eq 3010) { Write-Warning "Windows requested a restart to complete driver staging." }
}

# Keep the verified INF payload and pinned root-device installer for the
# privileged service. Device creation is deferred until a trusted session has
# passed Android attestation and received a short-lived display lease.
Get-ChildItem -LiteralPath (Split-Path -Parent $infs[0].FullName) -Force | Copy-Item -Destination $PayloadDirectory -Recurse -Force
Write-Host "Downloading the pinned NefCon root-device installer (not executed during setup)..."
Invoke-WebRequest -Uri $nefconUrl -OutFile $nefconArchive -UseBasicParsing
$nefconHash = (Get-FileHash -LiteralPath $nefconArchive -Algorithm SHA256).Hash.ToLowerInvariant()
if ($nefconHash -ne $expectedNefconSha256) { throw "NefCon archive SHA-256 mismatch. Expected $expectedNefconSha256; received $nefconHash. Nothing was executed." }
Expand-Archive -LiteralPath $nefconArchive -DestinationPath $nefconExtract -Force
$nefcon = Join-Path $nefconExtract "x64\\nefconw.exe"
if (-not (Test-Path -LiteralPath $nefcon -PathType Leaf)) { throw "The pinned NefCon archive does not contain x64\\nefconw.exe." }
Copy-Item -LiteralPath $nefcon -Destination (Join-Path $PayloadDirectory "nefconw.exe") -Force

$stagedInf = Get-ChildItem -LiteralPath $PayloadDirectory -Recurse -File -Filter "MttVDD.inf" | Select-Object -First 1
if ($null -eq $stagedInf -or -not (Test-Path -LiteralPath (Join-Path $PayloadDirectory "nefconw.exe"))) {
    throw "The deferred VDD activation payload is incomplete."
}
Write-Host "SUCCESS: signed VDD package staged and activation payload installed."
Write-Host "No ROOT\\MttVDD device was created; no virtual monitor was attached during setup."
Write-Host "Bridge will create the device only after a trusted Second Screen session is authorized."
