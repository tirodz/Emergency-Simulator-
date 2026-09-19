# Build Emergency-Simulator.exe on Windows.
#
#   powershell -ExecutionPolicy Bypass -File packaging\build_windows.ps1
#
# Produces dist\Emergency-Simulator.exe. Requires Python 3.10+ (with tkinter, which the official
# python.org installer includes) and internet access for PyInstaller's first install.

[CmdletBinding()]
param(
    [switch]$SkipTests,
    [switch]$NoVenv
)

$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $RepoRoot
Write-Host "Repository root: $RepoRoot"

function Get-PythonLauncher {
    foreach ($candidate in @("py", "python")) {
        if (Get-Command $candidate -ErrorAction SilentlyContinue) { return $candidate }
    }
    throw "Python was not found on PATH. Install it from https://www.python.org/downloads/windows/ (the installer includes tkinter)."
}

$Python = Get-PythonLauncher
Write-Host "Using launcher: $Python"

if (-not $NoVenv) {
    if (-not (Test-Path ".venv\Scripts\python.exe")) {
        Write-Host "Creating a virtual environment in .venv"
        & $Python -m venv .venv
    }
    $PythonExe = Join-Path $RepoRoot ".venv\Scripts\python.exe"
} else {
    $PythonExe = (Get-Command python).Source
}

Write-Host "Python: $($PythonExe)"
& $PythonExe -c "import tkinter; print('tkinter', tkinter.TkVersion)"

Write-Host "Installing build dependencies"
& $PythonExe -m pip install --upgrade pip
& $PythonExe -m pip install pyinstaller

if (-not $SkipTests) {
    Write-Host "Running the controller safety tests"
    & $PythonExe "tools\test_controller.py"
    if ($LASTEXITCODE -ne 0) { throw "Safety tests failed; refusing to build." }
}

Write-Host "Building the executable"
& $PythonExe -m PyInstaller --clean --noconfirm "packaging\Emergency-Simulator.spec"
if ($LASTEXITCODE -ne 0) { throw "PyInstaller failed." }

$Exe = Join-Path $RepoRoot "dist\Emergency-Simulator.exe"
if (-not (Test-Path $Exe)) { throw "Build finished but $Exe is missing." }

Write-Host ""
Write-Host "Built: $Exe"
Write-Host ("Size:  {0:N1} MB" -f ((Get-Item $Exe).Length / 1MB))
Write-Host ""
Write-Host "To run it, adb must be available. Set ADB_PATH if adb is not on PATH:"
Write-Host "  `$env:ADB_PATH = 'C:\platform-tools\adb.exe'"
Write-Host "  .\dist\Emergency-Simulator.exe"