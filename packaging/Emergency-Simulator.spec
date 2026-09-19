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
import os

REPO_ROOT = Path(SPECPATH).resolve().parent
APP_DIR = REPO_ROOT / "app"

#: The subset of Platform Tools this application can actually use.
#:
#: Platform Tools is a distribution of several separate command-line tools. Only adb drives a device,
#: and this application never flashes, formats or repartitions anything. Shipping the rest was pure
#: extra attack surface in a bundle whose whole job is to be trusted: it put `fastboot`, `mke2fs` and
#: a `sqlite3` shell inside an unsigned archive that unpacks itself at run time. Only these four
#: files are reachable: adb, its two companion DLLs (absent either, adb fails to load) and its
#: threaded-C runtime dependency.
PLATFORM_TOOLS_NEEDED = (
    "adb.exe",
    "AdbWinApi.dll",
    "AdbWinUsbApi.dll",
    "libwinpthread-1.dll",
)

datas = []
bundled_notes = []

#: When set, produce a build with no device-driving components at all: no injector, no adb. The
#: interface still runs, reports adb as MISSING, and cannot reach a device. This exists so the window
#: can be seen and reviewed on a machine where an unsigned executable that carries a device-console
#: binary is exactly the thing that should not be executed.
GUI_ONLY = bool(os.environ.get("EMERGENCY_SIMULATOR_GUI_ONLY"))

# -- the Android injector ---------------------------------------------------
#
# The remote path on the device is fixed, but the *local* path inside the bundle must match what
# app/runtime.py looks for: <root>/android/alertinject/out/alertinject.jar
injector_jar = REPO_ROOT / "android" / "alertinject" / "out" / "alertinject.jar"
if GUI_ONLY:
    bundled_notes.append("injector: NOT BUNDLED (GUI-only build)")
elif injector_jar.is_file():
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
# third-party binary distribution. CI records the final executable hash; when a download is needed it uses Google's current archive.
platform_tools = REPO_ROOT / "packaging" / "platform-tools"
if GUI_ONLY:
    bundled_notes.append("platform-tools: NOT BUNDLED (GUI-only build)")
elif platform_tools.is_dir() and any(platform_tools.iterdir()):
    staged = sorted(p.name for p in platform_tools.iterdir() if p.is_file())
    missing = [name for name in PLATFORM_TOOLS_NEEDED if name not in staged]
    if missing:
        raise SystemExit(
            "Staged Platform Tools are incomplete, so the bundled adb would fail to start.\n"
            f"  missing: {', '.join(missing)}\n"
            f"  staged : {', '.join(staged) or '(nothing)'}\n"
            "Re-stage them with packaging/build_windows.ps1."
        )
    # Only the needed files are bundled; the rest of the distribution is deliberately left out.
    for name in PLATFORM_TOOLS_NEEDED:
        datas.append((str(platform_tools / name), "platform-tools"))
    bundled_notes.append("platform-tools: " + ", ".join(PLATFORM_TOOLS_NEEDED))
    skipped = [n for n in staged if n not in PLATFORM_TOOLS_NEEDED]
    if skipped:
        bundled_notes.append("platform-tools: NOT bundled (unused by this tool): " + ", ".join(skipped))
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
    name="Emergency-Simulator-GUI-only" if GUI_ONLY else "Emergency-Simulator",
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
    # A version resource and a manifest that states the requested execution level. Both are identity:
    # they tell a scanner what this binary is and that it does not ask for elevation. Their absence is
    # one of the things that makes an unsigned, self-unpacking executable score badly.
    version=str(REPO_ROOT / "packaging" / "version_info.txt"),
    uac_admin=False,
)

print("Emergency-Simulator build contents:")
for note in bundled_notes:
    print(f"  {note}")