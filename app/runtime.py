"""Where the application's runtime files live, and how they are found.

A released Windows build must not require the user to install anything: not Python, not the Android
SDK, not a JDK. The Android injector and the platform tools are therefore carried inside the
distribution, and this module is the single place that knows how to locate them.

Two layouts are supported, and they are genuinely different:

**Packaged build.** PyInstaller unpacks the archive into a temporary directory exposed as
``sys._MEIPASS``. Bundled resources are found there.

**Repository checkout.** Development runs straight from the tree, so resources sit next to the code.

Keeping both in one module means the controller never has to ask which one it is running in, which is
what let the earlier packaged build get the paths subtly wrong.
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
from dataclasses import dataclass, field
from enum import Enum
from pathlib import Path
from typing import List, Optional

#: Name of the directory bundled platform tools are unpacked into.
PLATFORM_TOOLS_DIRNAME = "platform-tools"

#: Directory holding the prebuilt injector, relative to a bundle root.
INJECTOR_SUBPATH = Path("android") / "alertinject"

#: The jar that must ship with a release. It is built once from the Android SDK and committed, so a
#: user never needs a JDK or the SDK to run the application.
INJECTOR_JAR_NAME = "alertinject.jar"

#: adb's companion libraries. adb.exe loads these from its own directory, so they must be extracted
#: next to it or the executable fails at runtime with an unhelpful loader error.
ADB_COMPANION_FILES = ("AdbWinApi.dll", "AdbWinUsbApi.dll")


class AdbMode(str, Enum):
    """Where the adb we would use came from. The UI reports this verbatim."""

    BUNDLED = "BUNDLED"
    EXTERNAL = "EXTERNAL"
    MISSING = "MISSING"


@dataclass
class AdbCandidate:
    """An adb executable we found, plus whether it actually runs."""

    path: str
    mode: AdbMode
    version: Optional[str] = None
    works: Optional[bool] = None
    problem: Optional[str] = None


@dataclass
class RuntimeLayout:
    """Resolved locations of everything the application needs at run time."""

    bundle_root: Path
    is_frozen: bool
    search_roots: List[Path] = field(default_factory=list)

    # -- injector ----------------------------------------------------------

    @property
    def injector_jar(self) -> Optional[Path]:
        for root in self.search_roots:
            candidate = root / INJECTOR_SUBPATH / "out" / INJECTOR_JAR_NAME
            if candidate.is_file():
                return candidate
        return None

    @property
    def injector_sources_present(self) -> bool:
        """True when this is a development checkout that can rebuild the injector."""
        return any(
            (root / INJECTOR_SUBPATH).is_dir()
            and any((root / INJECTOR_SUBPATH).rglob("*.java"))
            for root in self.search_roots
        )

    # -- platform tools ----------------------------------------------------

    def bundled_adb(self) -> Optional[Path]:
        exe = "adb.exe" if os.name == "nt" else "adb"
        for root in self.search_roots:
            candidate = root / PLATFORM_TOOLS_DIRNAME / exe
            if candidate.is_file():
                return candidate
        return None

    def bundled_platform_tools_dir(self) -> Optional[Path]:
        adb = self.bundled_adb()
        return adb.parent if adb else None


def _iter_search_roots() -> List[Path]:
    """Directories to search for bundled resources, most specific first."""
    roots: List[Path] = []

    # PyInstaller's unpack directory. In a onefile build this is where datas land.
    meipass = getattr(sys, "_MEIPASS", None)
    if meipass:
        roots.append(Path(meipass))

    if getattr(sys, "frozen", False):
        # Resources are normally unpacked to _MEIPASS, but a directory-mode build (or a future
        # release that ships files loose beside the executable) puts them next to the binary.
        roots.append(Path(sys.executable).resolve().parent)
    else:
        # Repository checkout: app/runtime.py -> repository root.
        roots.append(Path(__file__).resolve().parent.parent)

    # De-duplicate while preserving order.
    seen = set()
    unique: List[Path] = []
    for root in roots:
        key = str(root)
        if key not in seen:
            seen.add(key)
            unique.append(root)
    return unique


def runtime_layout() -> RuntimeLayout:
    """Resolve the layout for the current process."""
    return RuntimeLayout(
        bundle_root=_iter_search_roots()[0],
        is_frozen=bool(getattr(sys, "frozen", False)),
        search_roots=_iter_search_roots(),
    )


# -- adb discovery ---------------------------------------------------------


def probe_adb(path: str, timeout: float = 15.0) -> AdbCandidate:
    """Run `adb version` so we know the executable genuinely works.

    A file check is not enough. On Windows adb.exe fails at load time if its companion DLLs are
    missing, which looks identical to "no adb" unless it is actually executed. Probing turns that
    into a specific, reportable error.
    """
    p = Path(path)
    if not p.is_file():
        return AdbCandidate(path=path, mode=AdbMode.MISSING, works=False, problem="file not found")

    try:
        proc = subprocess.run(
            [str(p), "version"],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
        )
    except subprocess.TimeoutExpired:
        return AdbCandidate(path=path, mode=AdbMode.MISSING, works=False, problem="timed out")
    except OSError as exc:
        return AdbCandidate(path=path, mode=AdbMode.MISSING, works=False, problem=f"cannot execute: {exc}")

    output = (proc.stdout or "") + (proc.stderr or "")
    if proc.returncode != 0:
        return AdbCandidate(
            path=path,
            mode=AdbMode.MISSING,
            works=False,
            problem=f"exited {proc.returncode}: {output.strip()[:200]}",
        )

    version = None
    for line in output.splitlines():
        if line.lower().startswith("android debug bridge"):
            version = line.strip()
            break
    return AdbCandidate(path=path, mode=AdbMode.MISSING, version=version, works=True)


def _external_adb_candidates(explicit: Optional[str] = None) -> List[str]:
    """Conventional adb locations owned by the user, not by us."""
    found: List[str] = []
    if explicit:
        found.append(explicit)

    env = os.environ.get("ADB_PATH")
    if env:
        p = Path(env)
        if p.is_dir():
            found.append(str(p / ("adb.exe" if os.name == "nt" else "adb")))
        else:
            found.append(str(p))

    which = shutil.which("adb")
    if which:
        found.append(which)

    exe = "adb.exe" if os.name == "nt" else "adb"
    sdk_roots: List[Path] = []
    for var in ("ANDROID_HOME", "ANDROID_SDK_ROOT"):
        val = os.environ.get(var)
        if val:
            sdk_roots.append(Path(val))
    local = os.environ.get("LOCALAPPDATA")
    if os.name == "nt" and local:
        sdk_roots.append(Path(local) / "Android" / "Sdk")
    sdk_roots.append(Path.home() / "Android" / "Sdk")
    if os.name != "nt":
        sdk_roots.append(Path("/opt/android-sdk"))
        sdk_roots.append(Path("/usr/lib/android-sdk"))
    for root in sdk_roots:
        found.append(str(root / "platform-tools" / exe))

    return found


def locate_adb(explicit: Optional[str] = None) -> AdbCandidate:
    """Find a usable adb, preferring the bundled copy.

    Bundled first because that is what makes a release self-contained and reproducible: the copy we
    shipped is the copy that was tested. An explicitly configured path still wins, so an advanced
    user can override with a newer platform-tools without us second-guessing them.
    """
    layout = runtime_layout()

    if explicit:
        cand = probe_adb(explicit)
        cand.mode = AdbMode.EXTERNAL if cand.works else AdbMode.MISSING
        return cand

    bundled = layout.bundled_adb()
    if bundled:
        cand = probe_adb(str(bundled))
        if cand.works:
            cand.mode = AdbMode.BUNDLED
            return cand
        # Present but unusable: report that specifically rather than silently falling through, so a
        # broken bundle is visible instead of being masked by an unrelated system adb.
        return AdbCandidate(
            path=str(bundled),
            mode=AdbMode.MISSING,
            works=False,
            problem=f"bundled adb is not usable ({cand.problem})",
        )

    for path in _external_adb_candidates():
        if not path or not Path(path).is_file():
            continue
        cand = probe_adb(path)
        if cand.works:
            cand.mode = AdbMode.EXTERNAL
            return cand

    return AdbCandidate(path="", mode=AdbMode.MISSING, works=False, problem="no adb executable found")


def persistent_dir() -> Path:
    """Directory for state that must survive across runs (settings, history).

    Never inside the bundle: a Windows release may be installed in a read-only location, and a
    onefile PyInstaller build unpacks to a temporary directory that is deleted on exit.
    """
    if os.name == "nt":
        base = os.environ.get("LOCALAPPDATA")
        if base:
            return Path(base) / "Emergency-Simulator"
    else:
        base = os.environ.get("XDG_STATE_HOME")
        if base:
            return Path(base) / "emergency-simulator"
    return Path.home() / ".emergency-simulator"


def adb_setup_hint() -> str:
    """Operator-facing explanation for a missing adb, used by the UI and the CLI."""
    return (
        "Android Platform Tools (adb) could not be found.\n\n"
        "This build should include a bundled copy. If it does not, you can:\n"
        "  1. download the official Platform Tools from\n"
        "     https://developer.android.com/tools/releases/platform-tools\n"
        "  2. set the ADB_PATH environment variable to the adb executable, or\n"
        "  3. choose the executable in Settings.\n\n"
        "No cellular transmission is involved at any point."
    )