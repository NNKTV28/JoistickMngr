<#
.SYNOPSIS
    Builds dist\JoystickManager-Setup-<version>.exe with Inno Setup.
.DESCRIPTION
    Expects target\release\JoystickManager.exe to be built (and signed) already.
    When SIGNING_CERTIFICATE_PATH is set, the installer and its uninstaller are signed too.
#>
param(
    [Parameter(Mandatory = $true)][string]$Version
)

$ErrorActionPreference = "Stop"
$repositoryRoot = Split-Path $PSScriptRoot -Parent

function Find-InnoSetupCompiler {
    $fromPath = Get-Command iscc.exe -ErrorAction SilentlyContinue
    if ($fromPath) { return $fromPath.Source }
    $knownLocations = @(
        (Join-Path ${env:ProgramFiles(x86)} "Inno Setup 6\ISCC.exe"),
        (Join-Path $env:ProgramFiles "Inno Setup 6\ISCC.exe"),
        (Join-Path $env:LOCALAPPDATA "Programs\Inno Setup 6\ISCC.exe")
    )
    $found = $knownLocations | Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $found) { throw "Inno Setup 6 was not found. Install it from https://jrsoftware.org/isinfo.php" }
    return $found
}

$compilerArguments = @("/DAppVersion=$Version")
if ($env:SIGNING_CERTIFICATE_PATH) {
    $signScript = Join-Path $PSScriptRoot "sign-file.ps1"
    $compilerArguments += "/DUseSignTool"
    $compilerArguments += "/Snikicoding=pwsh -NoProfile -ExecutionPolicy Bypass -File `"$signScript`" -FilePath `$f"
}
$compilerArguments += (Join-Path $repositoryRoot "installer\JoystickManager.iss")

& (Find-InnoSetupCompiler) @compilerArguments
if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed." }
