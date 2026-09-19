# -*- mode: python ; coding: utf-8 -*-
"""PyInstaller build definition for Emergency-Simulator.

Produces a single-file executable that starts the Tkinter interface and carries everything it needs
to talk to a device.

Build with:

    pyinstaller packaging/Emergency-Simulator.spec

Two resources are bundled, so a released executable needs no Python, no Android SDK and no JDK on the
machine it runs on:

1. **The Android injector** (``android/alertinject/out/alertinject.jar``). This is a dex jar that is
   pushed to the target device at run time. It is prebuilt and committed, so a user never compiles
   anything. Without it the tool cannot drive a device at all, so its absence fails the build.

2. **Android Platform Tools** (``packaging/platform-tools/``), when staged there by the build script.
   A release that carries its own adb is self-contained and, more importantly, reproducible: the copy
   that was tested is the copy that runs. When the directory is absent the executable still builds
   and falls back to an adb on the machine, which is the development case.

Each resource is placed at the path ``app.runtime`` searches for it, so the application finds them
through one code path in both a packaged build and a checkout.
"""

from pathlib import Path

REPO_ROOT = Path(SPECPATH).resolve().parent
APP_DIR = REPO_ROOT / "app"

datas = []
bundled_notes = []

# -- the Android injector ---------------------------------------------------
#
# The remote path on the device is fixed, but the *local* path inside the bundle must match what
# app/runtime.py looks for: <root>/android/alertinject/out/alertinject.jar
injector_jar = REPO_ROOT / "android" / "alertinject" / "out" / "alertinject.jar"
if injector_jar.is_file():
    datas.append((str(injector_jar), "android/alertinject/out"))
    bundled_notes.append(f"injector: {injector_jar.stat().st_size} bytes")
else:
    raise SystemExit(
        "The Android injector is missing, so this build would be unable to drive any device.\n"
        f"  Expected: {injector_jar}\n"
        "  Build it once with android/alertinject/build.sh (requires the Android SDK and a JDK),\n"
        "  then commit the jar. It is a build input for every release."
    )

# -- platform tools ---------------------------------------------------------
#
# Staged by packaging/build_windows.ps1, either downloaded from Google's official repository or
# copied from an existing Android SDK installation. Not committed to the repository: it is a
# third-party binary distribution, and pinning it at build time is what makes a release reproducible.
platform_tools = REPO_ROOT / "packaging" / "platform-tools"
if platform_tools.is_dir() and any(platform_tools.iterdir()):
    datas.append((str(platform_tools), "platform-tools"))
    bundled = sorted(p.name for p in platform_tools.iterdir() if p.is_file())
    bundled_notes.append("platform-tools: " + ", ".join(bundled))
else:
    bundled_notes.append(
        "platform-tools: NOT bundled (an adb on the target machine will be used)"
    )

hiddenimports = [
    "app.adb",
    "app.controller",
    "app.logsetup",
    "app.models",
    "app.runtime",
    "app.ui",
    "app.widgets",
    "tkinter",
    "tkinter.ttk",
    "tkinter.messagebox",
    "tkinter.filedialog",
]

a = Analysis(
    [str(APP_DIR / "main.py")],
    pathex=[str(REPO_ROOT), str(APP_DIR)],
    binaries=[],
    datas=datas,
    hiddenimports=hiddenimports,
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=[
        # Keep the binary small; none of these are used.
        "numpy", "pandas", "matplotlib", "PIL", "pytest", "setuptools",
    ],
    noarchive=False,
    optimize=0,
)

pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    a.binaries,
    a.datas,
    [],
    name="Emergency-Simulator",
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=False,
    upx_exclude=[],
    runtime_tmpdir=None,
    console=False,          # GUI application: no console window
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
)

print("Emergency-Simulator build contents:")
for note in bundled_notes:
    print(f"  {note}")