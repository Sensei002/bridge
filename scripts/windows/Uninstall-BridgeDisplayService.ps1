[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
$serviceName = "BridgeDisplayService"

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw "Run this script from an elevated PowerShell window (Run as administrator)."
}

$service = Get-Service -Name $serviceName -ErrorAction SilentlyContinue
if ($null -eq $service) {
    Write-Host "Bridge Display Service is not installed."
    exit 0
}

Write-Warning "Disconnect the Bridge virtual display in the Bridge desktop app before removing the service."
if ($service.Status -eq "Running") {
    Stop-Service -Name $serviceName -Force
    $service.WaitForStatus([ServiceProcess.ServiceControllerStatus]::Stopped, [TimeSpan]::FromSeconds(20))
}

& "$env:SystemRoot\System32\sc.exe" delete $serviceName
if ($LASTEXITCODE -ne 0) {
    throw "Could not remove Bridge Display Service (sc.exe exit code $LASTEXITCODE)."
}

Write-Host "Bridge Display Service removal requested."
Write-Host "The IddCx driver package is intentionally left installed; remove it separately only after confirming no Bridge virtual monitor is active."
