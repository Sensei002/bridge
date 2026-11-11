[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$ServiceExecutable,
    [switch]$AllowUnsignedDevelopmentBuild
)
$ErrorActionPreference = "Stop"
$serviceName = "BridgeDisplayService"
$displayName = "Bridge Display Service"
$installDirectory = Join-Path $env:ProgramFiles "Bridge\DisplayService"
$installedExecutable = Join-Path $installDirectory "bridge-display-service.exe"
function Assert-Administrator {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw "Run this script from an elevated PowerShell window (Run as administrator)."
    }
}
function Invoke-Sc {
    param([Parameter(Mandatory = $true)][string[]]$Arguments)
    & "$env:SystemRoot\System32\sc.exe" @Arguments
    if ($LASTEXITCODE -ne 0) { throw "sc.exe $($Arguments -join ' ') failed with exit code $LASTEXITCODE." }
}
function Set-ProtectedInstallAcl {
    param([Parameter(Mandatory = $true)][string]$Path)
    # Well-known SIDs keep this compatible with non-English Windows installations.
    & "$env:SystemRoot\System32\icacls.exe" $Path /inheritance:r /grant:r "*S-1-5-18:(OI)(CI)(F)" "*S-1-5-32-544:(OI)(CI)(F)" "*S-1-5-32-545:(OI)(CI)(RX)" | Out-Host
    if ($LASTEXITCODE -ne 0) { throw "Could not apply the protected Bridge service directory ACL (icacls exit code $LASTEXITCODE)." }
}
Assert-Administrator
$resolvedExecutable = (Resolve-Path -LiteralPath $ServiceExecutable -ErrorAction Stop).Path
if ([IO.Path]::GetExtension($resolvedExecutable) -ne ".exe" -or [IO.Path]::GetFileName($resolvedExecutable) -ne "bridge-display-service.exe") {
    throw "ServiceExecutable must point to bridge-display-service.exe."
}
$signature = Get-AuthenticodeSignature -LiteralPath $resolvedExecutable
if ($signature.Status -ne "Valid" -and -not $AllowUnsignedDevelopmentBuild) {
    throw "The service executable is not Authenticode-valid. Use a properly signed build, or explicitly pass -AllowUnsignedDevelopmentBuild for a trusted local development artifact."
}
if ($signature.Status -ne "Valid") { Write-Warning "Installing an unsigned development service build. Only use binaries built from trusted Bridge source." }
$existing = Get-Service -Name $serviceName -ErrorAction SilentlyContinue
if ($null -ne $existing -and $existing.Status -eq "Running") {
    Stop-Service -Name $serviceName -Force
    $existing.WaitForStatus([ServiceProcess.ServiceControllerStatus]::Stopped, [TimeSpan]::FromSeconds(20))
}
New-Item -ItemType Directory -Path $installDirectory -Force | Out-Null
Set-ProtectedInstallAcl -Path (Split-Path -Parent $installDirectory)
Set-ProtectedInstallAcl -Path $installDirectory
# LocalSystem must never execute a service binary from a user-writable Downloads/AppData directory.
Copy-Item -LiteralPath $resolvedExecutable -Destination $installedExecutable -Force
$sourceHash = (Get-FileHash -LiteralPath $resolvedExecutable -Algorithm SHA256).Hash
$installedHash = (Get-FileHash -LiteralPath $installedExecutable -Algorithm SHA256).Hash
if ($sourceHash -ne $installedHash) { throw "The installed service executable does not match the verified source file." }
$binaryPath = '"' + $installedExecutable + '"'
if ($null -eq $existing) {
    Invoke-Sc -Arguments @("create", $serviceName, "binPath=", $binaryPath, "start=", "auto", "obj=", "LocalSystem", "DisplayName=", $displayName)
} else {
    Invoke-Sc -Arguments @("config", $serviceName, "binPath=", $binaryPath, "start=", "auto", "obj=", "LocalSystem", "DisplayName=", $displayName)
}
& "$env:SystemRoot\System32\sc.exe" description $serviceName "Bridge privileged local service for authenticated virtual-display operations."
if ($LASTEXITCODE -ne 0) { throw "Could not set the Bridge Display Service description (exit code $LASTEXITCODE)." }
Invoke-Sc -Arguments @("failure", $serviceName, "reset=", "86400", "actions=", "restart/5000/restart/15000/restart/30000")
Start-Service -Name $serviceName
$service = Get-Service -Name $serviceName
$service.WaitForStatus([ServiceProcess.ServiceControllerStatus]::Running, [TimeSpan]::FromSeconds(20))
Write-Host "Bridge Display Service installed and running."
Write-Host "Executable: $installedExecutable"
Write-Host "Directory ACL: SYSTEM/Administrators full control; Users read/execute."
Write-Host "Account: LocalSystem | Startup: Automatic"
