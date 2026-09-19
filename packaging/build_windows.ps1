# Build Emergency-Simulator.exe on Windows.
#
#   powershell -ExecutionPolicy Bypass -File packaging\build_windows.ps1
#
# Produces dist\Emergency-Simulator.exe, a single-file executable that carries the Tkinter interface,
# the Android injector and its own copy of Android Platform Tools. Requires Python 3.10+ (with
# tkinter, which the official python.org installer includes).
#
# Platform Tools are staged into packaging\platform-tools and bundled. They are not committed: they
# are a third-party binary distribution. Pass -SkipPlatformTools to build against a system adb
# instead, which is useful when iterating but produces a build that is not self-contained.

[CmdletBinding()]
param(
    [switch]$SkipTests,
    [switch]$NoVenv,
    [switch]$SkipPlatformTools,
    [string]$PlatformToolsVersion = "36.0.0"
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

# -- the Android injector ---------------------------------------------------
# A release cannot drive a device without it, so its absence is fatal rather than a warning.
$Injector = Join-Path $RepoRoot "android\alertinject\out\alertinject.jar"
if (-not (Test-Path $Injector)) {
    throw @"
The Android injector is missing: $Injector

It is committed to the repository. If it is absent, restore it with:
    git checkout -- android/alertinject/out/alertinject.jar
or rebuild it (needs the Android SDK and a JDK):
    bash android/alertinject/build.sh
"@
}
Write-Host ("Injector: {0:N0} bytes" -f (Get-Item $Injector).Length)

# -- platform tools ---------------------------------------------------------
$Stage = Join-Path $RepoRoot "packaging\platform-tools"
if ($SkipPlatformTools) {
    Write-Host "Platform Tools: skipped (-SkipPlatformTools); the build will use a system adb"
} else {
    if (Test-Path $Stage) { Remove-Item -Recurse -Force $Stage }
    New-Item -ItemType Directory -Path $Stage -Force | Out-Null

    # Prefer a copy already present on this machine: it is faster and offline.
    $LocalAdb = $null
    foreach ($candidate in @(
        (Join-Path $env:LOCALAPPDATA "Android\Sdk\platform-tools\adb.exe"),
        (Join-Path $env:ANDROID_HOME "platform-tools\adb.exe"),
        (Join-Path $env:ANDROID_SDK_ROOT "platform-tools\adb.exe")
    )) {
        if ($candidate -and (Test-Path $candidate)) { $LocalAdb = $candidate; break }
    }
    if (-not $LocalAdb) {
        $onPath = Get-Command adb -ErrorAction SilentlyContinue
        if ($onPath) { $LocalAdb = $onPath.Source }
    }

    if ($LocalAdb) {
        Write-Host "Platform Tools: copying from $LocalAdb"
        Copy-Item (Join-Path (Split-Path -Parent $LocalAdb) "*") -Destination $Stage -Recurse -Force
    } else {
        $Url = "https://dl.google.com/android/repository/platform-tools-latest-windows.zip"
        $Zip = Join-Path $env:TEMP "platform-tools-$PlatformToolsVersion.zip"
        Write-Host "Platform Tools: downloading $Url"
        Invoke-WebRequest -Uri $Url -OutFile $Zip -UseBasicParsing
        Expand-Archive -Path $Zip -DestinationPath (Join-Path $env:TEMP "platform-tools-stage") -Force
        Copy-Item (Join-Path $env:TEMP "platform-tools-stage\platform-tools\*") -Destination $Stage -Recurse -Force
    }

    $AdbExe = Join-Path $Stage "adb.exe"
    if (-not (Test-Path $AdbExe)) { throw "adb.exe was not staged into $Stage" }
    foreach ($dll in @("AdbWinApi.dll", "AdbWinUsbApi.dll")) {
        if (-not (Test-Path (Join-Path $Stage $dll))) {
            throw "$dll is missing beside adb.exe. adb will fail to load without it."
        }
    }
    & $AdbExe version
    if ($LASTEXITCODE -ne 0) { throw "The staged adb.exe does not run." }
    Write-Host "Platform Tools: staged and verified"
}

# -- dependencies -----------------------------------------------------------
Write-Host "Installing build dependencies"
& $PythonExe -m pip install --upgrade pip
& $PythonExe -m pip install pyinstaller

if (-not $SkipTests) {
    Write-Host "Running the controller safety tests"
    & $PythonExe "tools\test_controller.py"
    if ($LASTEXITCODE -ne 0) { throw "Safety tests failed; refusing to build." }
}

# -- build ------------------------------------------------------------------
Write-Host "Building the executable"
& $PythonExe -m PyInstaller --clean --noconfirm "packaging\Emergency-Simulator.spec"
if ($LASTEXITCODE -ne 0) { throw "PyInstaller failed." }

$Exe = Join-Path $RepoRoot "dist\Emergency-Simulator.exe"
if (-not (Test-Path $Exe)) { throw "Build finished but $Exe is missing." }

Write-Host ""
Write-Host "Built: $Exe"
Write-Host ("Size:  {0:N1} MB" -f ((Get-Item $Exe).Length / 1MB))

# -- verification -----------------------------------------------------------
# A frozen build must be able to resolve its own resources. This runs the executable with no device
# attached and reads the layout it reports, which is the check that catches a bundle which merely
# built successfully.
Write-Host ""
Write-Host "Verifying the frozen build resolves its bundled resources"
Push-Location (Join-Path $RepoRoot "dist")
try {
    # The executable is a GUI application with no console, so the self-test writes a file. Ask for one
    # explicitly rather than relying on stdout, which is not attached in a windowed build. Start-Process
    # -Wait is also required: a GUI-subsystem binary returns immediately when invoked directly, and the
    # report would be read before the process had written it.
    $Report = Join-Path $env:TEMP "emergency-simulator-selftest.txt"
    Remove-Item $Report -ErrorAction SilentlyContinue
    $Proc = Start-Process -FilePath $Exe -ArgumentList "--selftest=$Report" -Wait -PassThru
    $SelfTestCode = $Proc.ExitCode
    if (-not (Test-Path $Report)) {
        throw "The self-test wrote no report to $Report (exit $SelfTestCode)."
    }
    $SelfTestText = Get-Content $Report -Raw
    Write-Host $SelfTestText
    if ($SelfTestCode -ne 0) { throw "The frozen build failed its self-test (exit $SelfTestCode)." }
    if ($SelfTestText -notmatch "injector\s*:\s*FOUND") {
        throw "The frozen build cannot locate its bundled injector."
    }
    if (-not $SkipPlatformTools -and $SelfTestText -notmatch "adb\s*:\s*BUNDLED") {
        throw "The frozen build did not resolve the bundled adb."
    }
} finally {
    Pop-Location
}

Write-Host ""
Write-Host "The build is self-contained. Run it with:" -ForegroundColor Green
Write-Host "  .\dist\Emergency-Simulator.exe"
Write-Host ""
Write-Host "To use an adb from elsewhere instead, set ADB_PATH:"
Write-Host "  `$env:ADB_PATH = 'C:\platform-tools\adb.exe'"