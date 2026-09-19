# Build the Windows installer locally.
#
# Prerequisite: a built dist\Emergency-Simulator.exe and Inno Setup 6 (ISCC.exe).
# Example:
#   powershell -ExecutionPolicy Bypass -File packaging\build_installer.ps1

[CmdletBinding()]
param(
    [string]$IsccPath = ""
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $RepoRoot

$Exe = Join-Path $RepoRoot "dist\Emergency-Simulator.exe"
if (-not (Test-Path $Exe)) {
    throw "dist\Emergency-Simulator.exe is missing. Build it first with packaging\build_windows.ps1."
}

if ($IsccPath) {
    $Compiler = $IsccPath
} else {
    $candidates = @(
        (Join-Path $env:LOCALAPPDATA "Programs\Inno Setup 6\ISCC.exe"),
        "C:\Program Files (x86)\Inno Setup 6\ISCC.exe",
        "C:\Program Files\Inno Setup 6\ISCC.exe"
    )
    $Compiler = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $Compiler) {
        $command = Get-Command ISCC.exe -ErrorAction SilentlyContinue
        if ($command) { $Compiler = $command.Source }
    }
}

if (-not $Compiler) {
    throw "ISCC.exe was not found. Install Inno Setup 6, then re-run this script."
}

& $Compiler "packaging\Emergency-Simulator.iss"
if ($LASTEXITCODE -ne 0) {
    throw "Inno Setup failed with exit code $LASTEXITCODE."
}

$Installer = Join-Path $RepoRoot "installer-output\Emergency-Simulator-Setup-1.1.0.exe"
if (-not (Test-Path $Installer)) {
    throw "Installer compilation finished without producing $Installer"
}

$Hash = (Get-FileHash $Installer -Algorithm SHA256).Hash
Write-Host ""
Write-Host "Installer: $Installer" -ForegroundColor Green
Write-Host "SHA-256 : $Hash"
Write-Host "Size    : $((Get-Item $Installer).Length) bytes"
