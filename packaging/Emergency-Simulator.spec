# -*- mode: python ; coding: utf-8 -*-
"""PyInstaller build definition for Emergency-Simulator.

Produces a single-file executable that starts the Tkinter interface. Build with:

    pyinstaller packaging/Emergency-Simulator.spec

The Android-side injector is deliberately NOT bundled. It is a dex jar that is built by the Android
SDK and pushed to the target device at run time, so the desktop binary stays a pure Python artefact.
"""

from pathlib import Path

REPO_ROOT = Path(SPECPATH).resolve().parent
APP_DIR = REPO_ROOT / "app"
TOOLS_DIR = REPO_ROOT / "tools"

# Collecting the whole app package keeps every module importable in the frozen build.
datas = []

hiddenimports = [
    "app.adb",
    "app.controller",
    "app.logsetup",
    "app.models",
    "app.ui",
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