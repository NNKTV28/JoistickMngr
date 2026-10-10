<#
.SYNOPSIS
    Signs a file with the NikiCoding code signing certificate.
.DESCRIPTION
    Reads the certificate from SIGNING_CERTIFICATE_PATH and SIGNING_CERTIFICATE_PASSWORD
    (set by prepare-signing-certificate.ps1) and signs with a SHA-256 timestamp.
#>
param(
    [Parameter(Mandatory = $true)][string]$FilePath
)

$ErrorActionPreference = "Stop"

function Find-SignTool {
    $fromPath = Get-Command signtool.exe -ErrorAction SilentlyContinue
    if ($fromPath) { return $fromPath.Source }
    $windowsKits = Join-Path ${env:ProgramFiles(x86)} "Windows Kits\10\bin"
    $candidates = Get-ChildItem $windowsKits -Recurse -Filter signtool.exe -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match "\\x64\\" } |
        Sort-Object FullName -Descending
    if (-not $candidates) { throw "signtool.exe was not found. Install the Windows SDK." }
    return $candidates[0].FullName
}

if (-not $env:SIGNING_CERTIFICATE_PATH) { throw "SIGNING_CERTIFICATE_PATH is not set. Run prepare-signing-certificate.ps1 first." }

$signTool = Find-SignTool
& $signTool sign `
    /f $env:SIGNING_CERTIFICATE_PATH `
    /p $env:SIGNING_CERTIFICATE_PASSWORD `
    /fd SHA256 `
    /tr "http://timestamp.digicert.com" `
    /td SHA256 `
    /d "Joystick Manager" `
    /du "https://nikicoding.com" `
    $FilePath
if ($LASTEXITCODE -ne 0) { throw "Signing failed for $FilePath" }
