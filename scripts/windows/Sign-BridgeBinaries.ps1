[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string[]]$Files,
    [string]$PublisherPinOutput
)

$ErrorActionPreference = "Stop"

$pfxBase64 = $env:BRIDGE_DRIVER_SIGNING_PFX_BASE64
$pfxPassword = $env:BRIDGE_DRIVER_SIGNING_PFX_PASSWORD
$expectedThumbprint = ($env:BRIDGE_DRIVER_CERT_THUMBPRINT -replace '\s', '').ToUpperInvariant()

if ([string]::IsNullOrWhiteSpace($pfxBase64) -or [string]::IsNullOrWhiteSpace($pfxPassword) -or [string]::IsNullOrWhiteSpace($expectedThumbprint)) {
    throw "Missing repository Actions secrets: BRIDGE_DRIVER_SIGNING_PFX_BASE64, BRIDGE_DRIVER_SIGNING_PFX_PASSWORD, or BRIDGE_DRIVER_CERT_THUMBPRINT. Configure them as repository secrets (not a reviewer-gated environment)."
}
if ($expectedThumbprint -notmatch '^[0-9A-F]{40}$') {
    throw "BRIDGE_DRIVER_CERT_THUMBPRINT must be a 40-character SHA-1 certificate thumbprint."
}

$pfxPath = Join-Path $env:RUNNER_TEMP ("bridge-signing-" + [guid]::NewGuid().ToString("N") + ".pfx")
$certificates = [System.Security.Cryptography.X509Certificates.X509Certificate2Collection]::new()
try {
    try {
        $pfxBytes = [Convert]::FromBase64String($pfxBase64)
    } catch {
        throw "BRIDGE_DRIVER_SIGNING_PFX_BASE64 is not valid Base64."
    }
    [IO.File]::WriteAllBytes($pfxPath, $pfxBytes)
    [Array]::Clear($pfxBytes, 0, $pfxBytes.Length)

    $flags = [System.Security.Cryptography.X509Certificates.X509KeyStorageFlags]::Exportable -bor [System.Security.Cryptography.X509Certificates.X509KeyStorageFlags]::EphemeralKeySet
    $certificates.Import($pfxPath, $pfxPassword, $flags)
    $certificate = $certificates | Where-Object { $_.Thumbprint -eq $expectedThumbprint } | Select-Object -First 1
    if ($null -eq $certificate) {
        throw "The PFX does not contain the certificate identified by BRIDGE_DRIVER_CERT_THUMBPRINT."
    }
    if (-not $certificate.HasPrivateKey) {
        throw "The configured signing certificate has no private key."
    }
    $codeSigningEku = $certificate.Extensions | Where-Object {
        $_ -is [System.Security.Cryptography.X509Certificates.X509EnhancedKeyUsageExtension]
    } | ForEach-Object { $_.EnhancedKeyUsages } | Where-Object { $_.Value -eq "1.3.6.1.5.5.7.3.3" }
    if ($null -eq $codeSigningEku) {
        throw "The configured certificate does not include the Code Signing EKU (1.3.6.1.5.5.7.3.3); it cannot sign Bridge executables."
    }
    $now = [DateTime]::UtcNow
    if ($now -lt $certificate.NotBefore.ToUniversalTime() -or $now -gt $certificate.NotAfter.ToUniversalTime()) {
        throw "The configured signing certificate is outside its validity period."
    }

    $sha256 = [Security.Cryptography.SHA256]::Create()
    try {
        $publisherPin = ([Convert]::ToHexString($sha256.ComputeHash($certificate.RawData))).ToUpperInvariant()
    } finally {
        $sha256.Dispose()
    }

    $sdkRoot = Join-Path ${env:ProgramFiles(x86)} "Windows Kits\10\bin"
    $signTool = Get-ChildItem -Path $sdkRoot -Filter signtool.exe -Recurse -ErrorAction Stop |
        Where-Object { $_.FullName -match '\\x64\\signtool\.exe$' } |
        Sort-Object FullName -Descending |
        Select-Object -First 1
    if ($null -eq $signTool) {
        throw "Windows SDK signtool.exe (x64) was not found on the hosted runner."
    }

    foreach ($file in $Files) {
        $resolved = (Resolve-Path -LiteralPath $file -ErrorAction Stop).Path
        if ([IO.Path]::GetExtension($resolved) -notin @(".exe", ".msi")) {
            throw "Refusing to sign unsupported file type: $resolved"
        }
        Write-Host "Signing $([IO.Path]::GetFileName($resolved)) with the configured Bridge publisher certificate."
        & $signTool.FullName sign /fd SHA256 /td SHA256 /tr "http://timestamp.digicert.com" /f $pfxPath /p $pfxPassword /sha1 $expectedThumbprint $resolved
        if ($LASTEXITCODE -ne 0) {
            throw "Authenticode signing failed for $resolved (signtool exit code $LASTEXITCODE)."
        }
        & $signTool.FullName verify /pa /all /v $resolved
        if ($LASTEXITCODE -ne 0) {
            throw "Authenticode verification failed for $resolved. The certificate may not chain to a trusted root on Windows."
        }
        $signature = Get-AuthenticodeSignature -LiteralPath $resolved
        if ($signature.Status -ne "Valid" -or $null -eq $signature.SignerCertificate) {
            throw "PowerShell did not validate the Authenticode signature for $resolved (status: $($signature.Status))."
        }
        $fileHash = ([Convert]::ToHexString($sha256.ComputeHash($signature.SignerCertificate.RawData))).ToUpperInvariant()
        if ($fileHash -ne $publisherPin) {
            throw "Signed file $resolved does not use the expected Bridge publisher certificate."
        }
    }

    if (-not [string]::IsNullOrWhiteSpace($PublisherPinOutput)) {
        $pinDirectory = Split-Path -Parent $PublisherPinOutput
        if (-not [string]::IsNullOrWhiteSpace($pinDirectory)) {
            New-Item -ItemType Directory -Force -Path $pinDirectory | Out-Null
        }
        [IO.File]::WriteAllText($PublisherPinOutput, $publisherPin + [Environment]::NewLine, [Text.Encoding]::ASCII)
        Write-Host "Wrote the verified desktop publisher SHA-256 pin."
    }
    Write-Host "All requested Bridge binaries passed Authenticode verification."
} finally {
    $certificates | ForEach-Object { $_.Dispose() }
    if (Test-Path -LiteralPath $pfxPath) {
        Remove-Item -LiteralPath $pfxPath -Force
    }
}
