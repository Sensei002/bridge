[CmdletBinding()]
param(
    [string]$OutputDirectory = (Join-Path $env:USERPROFILE "BridgeSigning")
)

$ErrorActionPreference = "Stop"

$subject = "CN=Bridge Project Driver Signing"
$pfxPath = Join-Path $OutputDirectory "Bridge-Driver-CodeSigning.pfx"
$cerPath = Join-Path $OutputDirectory "Bridge-Driver-CodeSigning.cer"

if (Test-Path $pfxPath) {
    throw "Refusing to overwrite existing private-key file: $pfxPath"
}
if (Test-Path $cerPath) {
    throw "Refusing to overwrite existing public certificate: $cerPath"
}

New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null

Write-Host "Creating a self-signed Bridge code-signing certificate."
Write-Host "This certificate is for Bridge package-signing experiments; it is not a Microsoft-issued production driver signature."
$password = Read-Host "Enter a strong PFX export password (use at least 20 characters)" -AsSecureString

$certificate = $null
try {
    $certificate = New-SelfSignedCertificate `
        -Type CodeSigningCert `
        -Subject $subject `
        -FriendlyName "Bridge Driver Signing - Self-Signed" `
        -CertStoreLocation "Cert:\CurrentUser\My" `
        -KeyAlgorithm RSA `
        -KeyLength 3072 `
        -KeySpec Signature `
        -HashAlgorithm SHA256 `
        -KeyExportPolicy Exportable `
        -NotAfter (Get-Date).AddYears(5)

    Export-PfxCertificate -Cert $certificate -FilePath $pfxPath -Password $password | Out-Null
    Export-Certificate -Cert $certificate -FilePath $cerPath -Type CERT | Out-Null

    $thumbprint = $certificate.Thumbprint
}
finally {
    if ($certificate) {
        Remove-Item -Path "Cert:\CurrentUser\My\$($certificate.Thumbprint)" -ErrorAction SilentlyContinue
    }
}

Write-Host ""
Write-Host "Bridge signing certificate created."
Write-Host "Private PFX: $pfxPath"
Write-Host "Public CER:  $cerPath"
Write-Host "Thumbprint:  $thumbprint"
Write-Host ""
Write-Host "IMPORTANT: Keep the PFX and its password private. Never commit or upload the PFX as a public artifact."
Write-Host "Next: add the PFX Base64 and password as GitHub Actions secrets, and the thumbprint as a repository variable."
