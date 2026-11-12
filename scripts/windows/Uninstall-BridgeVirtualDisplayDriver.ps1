[CmdletBinding(SupportsShouldProcess = $true)]
param()
$ErrorActionPreference = "Stop"
function Assert-Administrator {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw "Bridge Virtual Display Driver removal requires administrator approval. Re-run this script elevated."
    }
}
Assert-Administrator
Import-Module Dism -ErrorAction Stop
$packages = @(Get-WindowsDriver -Online -All | Where-Object { $_.OriginalFileName -match '(?i)\\MttVDD\.inf$' })
if ($packages.Count -eq 0) { Write-Host "The Bridge upstream virtual display driver is not installed."; return }
if (-not $PSCmdlet.ShouldProcess("VirtualDrivers MttVDD driver package(s)", "Disable virtual display and remove from Driver Store")) { return }
# The upstream pipe is local-only and accepts a UTF-16, NUL-terminated command.
# It has no acknowledgement, so this only requests removal of the active monitor.
$pipe = [IO.Pipes.NamedPipeClientStream]::new(".", "MTTVirtualDisplayPipe", [IO.Pipes.PipeDirection]::Out)
try {
    $pipe.Connect(1200)
    $payload = [Text.Encoding]::Unicode.GetBytes("SETDISPLAYCOUNT 0" + [char]0)
    $pipe.Write($payload, 0, $payload.Length)
    $pipe.Flush()
} catch [TimeoutException] {
    Write-Warning "The upstream VDD control pipe was unavailable. Continuing with package removal; Windows may require a restart if a monitor is still active."
} finally { $pipe.Dispose() }
$pnputil = Join-Path $env:SystemRoot "System32\pnputil.exe"
foreach ($package in $packages) {
    $publishedName = [IO.Path]::GetFileName($package.Driver)
    if ($publishedName -notmatch '^oem[0-9]+\.inf$') { throw "Windows returned unexpected published driver name '$publishedName'; refusing removal." }
    & $pnputil /delete-driver $publishedName /uninstall
    $exitCode = $LASTEXITCODE
    if ($exitCode -notin @(0, 3010)) { throw "Could not remove $publishedName (pnputil exit code $exitCode)." }
    if ($exitCode -eq 3010) { Write-Warning "Windows requested a restart to finish removing $publishedName." }
}
Write-Host "Bridge upstream virtual display driver package removal completed."
