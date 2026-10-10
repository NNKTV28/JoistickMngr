<#
.SYNOPSIS
    Creates a long-lived self-signed NikiCoding code signing certificate and stores it as GitHub secrets.
.DESCRIPTION
    Every release is then signed with the same certificate. The .pfx file is kept outside the repository.
    A self-signed certificate is only trusted on computers where it is installed; for a publisher name that
    Windows shows to everyone, replace the secrets with a certificate from a public certificate authority.
.PARAMETER TrustOnThisComputer
    Also installs the certificate as trusted on this PC, so Windows shows NikiCoding as the publisher here.
#>
param(
    [string]$Repository = "NNKTV28/JoistickMngr",
    [switch]$TrustOnThisComputer
)

$ErrorActionPreference = "Stop"

$outputDirectory = Join-Path $env:APPDATA "NikiCoding\signing"
New-Item -ItemType Directory -Force $outputDirectory | Out-Null
$certificatePath = Join-Path $outputDirectory "nikicoding-code-signing.pfx"
$publicCertificatePath = Join-Path $outputDirectory "nikicoding-code-signing.cer"
$password = [System.Guid]::NewGuid().ToString("N") + [System.Guid]::NewGuid().ToString("N")

$certificate = New-SelfSignedCertificate `
    -Type CodeSigningCert `
    -Subject "CN=NikiCoding, O=NikiCoding" `
    -FriendlyName "NikiCoding Code Signing" `
    -KeyAlgorithm RSA -KeyLength 3072 `
    -KeyExportPolicy Exportable `
    -CertStoreLocation Cert:\CurrentUser\My `
    -NotAfter (Get-Date).AddYears(5)

Export-PfxCertificate -Cert $certificate -FilePath $certificatePath -Password (ConvertTo-SecureString $password -AsPlainText -Force) | Out-Null
Export-Certificate -Cert $certificate -FilePath $publicCertificatePath | Out-Null

[System.Convert]::ToBase64String([System.IO.File]::ReadAllBytes($certificatePath)) | gh secret set SIGNING_CERTIFICATE_BASE64 --repo $Repository
$password | gh secret set SIGNING_CERTIFICATE_PASSWORD --repo $Repository

if ($TrustOnThisComputer) {
    Import-Certificate -FilePath $publicCertificatePath -CertStoreLocation Cert:\LocalMachine\Root | Out-Null
    Import-Certificate -FilePath $publicCertificatePath -CertStoreLocation Cert:\LocalMachine\TrustedPublisher | Out-Null
}

Write-Output "Certificate thumbprint: $($certificate.Thumbprint)"
Write-Output "Private key saved to $certificatePath (keep it safe, never commit it)."
Write-Output "GitHub secrets SIGNING_CERTIFICATE_BASE64 and SIGNING_CERTIFICATE_PASSWORD were updated for $Repository."
