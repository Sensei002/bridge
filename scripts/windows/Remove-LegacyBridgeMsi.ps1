$ErrorActionPreference = "Stop"
$uninstallRoot = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall"
if (-not (Test-Path -LiteralPath $uninstallRoot)) { exit 0 }

$legacy = @(Get-ChildItem -LiteralPath $uninstallRoot -ErrorAction SilentlyContinue | ForEach-Object {
    Get-ItemProperty -LiteralPath $_.PSPath -ErrorAction SilentlyContinue
} | Where-Object {
    $_.DisplayName -eq "Bridge" -and $_.Publisher -eq "Bridge Project" -and $_.WindowsInstaller -eq 1
})

foreach ($entry in $legacy) {
    $match = [regex]::Match([string]$entry.UninstallString, '\{[0-9A-Fa-f-]{36}\}')
    if (-not $match.Success) {
        Write-Warning "Found an older Bridge MSI but could not safely identify its product code. Leaving it installed."
        continue
    }
    Write-Host "Removing the previous per-user Bridge MSI installation..."
    $process = Start-Process -FilePath "$env:SystemRoot\System32\msiexec.exe" -ArgumentList @("/x", $match.Value, "/qn", "/norestart") -Wait -PassThru
    if ($process.ExitCode -notin @(0, 1605, 3010)) {
        throw "Previous Bridge MSI removal failed with exit code $($process.ExitCode)."
    }
}
