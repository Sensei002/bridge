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
    if ($LASTEXITCODE -ne 0) {
        throw "sc.exe $($Arguments -join ' ') failed with exit code $LASTEXITCODE."
    }
}

Assert-Administrator

$resolvedExecutable = (Resolve-Path -LiteralPath $ServiceExecutable -ErrorAction Stop).Path
if ([IO.Path]::GetExtension($resolvedExecutable) -ne ".exe") {
    throw "ServiceExecutable must point to bridge-display-service.exe."
}
if ([IO.Path]::GetFileName($resolvedExecutable) -ne "bridge-display-service.exe") {
    throw "Unexpected service executable name. Expected bridge-display-service.exe."
}

$signature = Get-AuthenticodeSignature -LiteralPath $resolvedExecutable
if ($signature.Status -ne "Valid" -and -not $AllowUnsignedDevelopmentBuild) {
    throw "The service executable is not Authenticode-valid. Use a properly signed build, or explicitly pass -AllowUnsignedDevelopmentBuild for a trusted local development artifact."
}
if ($signature.Status -ne "Valid") {
    Write-Warning "Installing an unsigned development service build. Only use binaries built from trusted Bridge source."
}

$existing = Get-Service -Name $serviceName -ErrorAction SilentlyContinue
if ($null -eq $existing) {
    $binaryPath = '"' + $resolvedExecutable + '"'
    Invoke-Sc -Arguments @(
        "create", $serviceName,
        "binPath=", $binaryPath,
        "start=", "auto",
        "obj=", "LocalSystem",
        "DisplayName=", $displayName
    )
} else {
    if ($existing.Status -eq "Running") {
        Stop-Service -Name $serviceName -Force
        $existing.WaitForStatus([ServiceProcess.ServiceControllerStatus]::Stopped, [TimeSpan]::FromSeconds(15))
    }
    $binaryPath = '"' + $resolvedExecutable + '"'
    Invoke-Sc -Arguments @(
        "config", $serviceName,
        "binPath=", $binaryPath,
        "start=", "auto",
        "obj=", "LocalSystem",
        "DisplayName=", $displayName
    )
}

& "$env:SystemRoot\System32\sc.exe" description $serviceName "Bridge privileged local service for authenticated virtual-display operations."
if ($LASTEXITCODE -ne 0) {
    throw "Could not set the Bridge Display Service description (exit code $LASTEXITCODE)."
}

# Recover from an unexpected service-process crash, but do not restart on clean stops.
Invoke-Sc -Arguments @(
    "failure", $serviceName,
    "reset=", "86400",
    "actions=", "restart/5000/restart/15000/restart/30000"
)

Start-Service -Name $serviceName
$service = Get-Service -Name $serviceName
$service.WaitForStatus([ServiceProcess.ServiceControllerStatus]::Running, [TimeSpan]::FromSeconds(20))

Write-Host "Bridge Display Service installed and running."
Write-Host "Executable: $resolvedExecutable"
Write-Host "Account: LocalSystem | Startup: Automatic"
