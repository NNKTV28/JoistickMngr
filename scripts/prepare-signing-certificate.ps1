<#
.SYNOPSIS
    Makes the NikiCoding signing certificate available to the build.
.DESCRIPTION
    Uses the SIGNING_CERTIFICATE_BASE64 / SIGNING_CERTIFICATE_PASSWORD secrets when they exist.
    Without them a temporary self-signed NikiCoding certificate is created, so every build is still signed.
    Exports SIGNING_CERTIFICATE_PATH and SIGNING_CERTIFICATE_PASSWORD for the following steps.
#>

$ErrorActionPreference = "Stop"

$workingDirectory = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { [System.IO.Path]::GetTempPath() }
$certificatePath = Join-Path $workingDirectory "nikicoding-signing.pfx"

function Save-CertificateFromSecret {
    [System.IO.File]::WriteAllBytes($certificatePath, [System.Convert]::FromBase64String($env:SIGNING_CERTIFICATE_BASE64))
    return $env:SIGNING_CERTIFICATE_PASSWORD
}

function New-TemporaryCertificate {
    $password = [System.Guid]::NewGuid().ToString("N")
    $certificate = New-SelfSignedCertificate `
        -Type CodeSigningCert `
        -Subject "CN=NikiCoding, O=NikiCoding" `
        -FriendlyName "NikiCoding Code Signing" `
        -KeyAlgorithm RSA -KeyLength 3072 `
        -CertStoreLocation Cert:\CurrentUser\My `
        -NotAfter (Get-Date).AddYears(3)
    $securePassword = ConvertTo-SecureString $password -AsPlainText -Force
    Export-PfxCertificate -Cert $certificate -FilePath $certificatePath -Password $securePassword | Out-Null
    Write-Warning "No signing secret found; signed with a temporary self-signed certificate."
    return $password
}

$certificatePassword = if ($env:SIGNING_CERTIFICATE_BASE64) { Save-CertificateFromSecret } else { New-TemporaryCertificate }

if ($env:GITHUB_ENV) {
    Write-Output "::add-mask::$certificatePassword"
    "SIGNING_CERTIFICATE_PATH=$certificatePath" | Out-File -FilePath $env:GITHUB_ENV -Append -Encoding utf8
    "SIGNING_CERTIFICATE_PASSWORD=$certificatePassword" | Out-File -FilePath $env:GITHUB_ENV -Append -Encoding utf8
} else {
    $env:SIGNING_CERTIFICATE_PATH = $certificatePath
    $env:SIGNING_CERTIFICATE_PASSWORD = $certificatePassword
}
