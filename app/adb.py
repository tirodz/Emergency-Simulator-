"""Thin, safe wrapper around the adb executable.

Every call goes through :func:`subprocess.run` with an argument *array*, never a shell string, so a
device serial or a message body can never be reinterpreted as shell syntax. Timeouts are mandatory.
"""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Dict, List, Optional, Sequence

from .models import Device, DeviceState

DEFAULT_TIMEOUT = 20.0
LONG_TIMEOUT = 180.0
BOOT_TIMEOUT = 300.0

# The on-device package that owns the emergency-alert machinery. Google's module and the AOSP one
# have different names, so both are probed.
CELLBROADCAST_PACKAGES = (
    "com.google.android.cellbroadcastreceiver",
    "com.android.cellbroadcastreceiver",
)

# Properties we read to describe a device. Keys are our field names.
DEVICE_PROPS = {
    "model": "ro.product.model",
    "product": "ro.product.device",
    "release": "ro.build.version.release",
    "sdk": "ro.build.version.sdk",
    "build_type": "ro.build.type",
    "debuggable": "ro.debuggable",
}


class AdbError(RuntimeError):
    """Raised when adb cannot be located or a command times out."""


@dataclass
class AdbResult:
    argv: List[str]
    returncode: int
    stdout: str
    stderr: str
    timed_out: bool = False

    @property
    def ok(self) -> bool:
        return self.returncode == 0 and not self.timed_out

    @property
    def output(self) -> str:
        return (self.stdout + "\n" + self.stderr).strip()


def find_adb(explicit: Optional[str] = None) -> Optional[str]:
    """Locate a usable adb without ever downloading it.

    Delegates to :mod:`app.runtime`, which owns the bundled-vs-external policy and verifies that the
    executable actually runs. Nothing here re-implements that search order, so the CLI, the GUI and
    the packaged build cannot drift apart on which adb they would pick.
    """
    from .runtime import AdbMode, locate_adb

    candidate = locate_adb(explicit)
    if candidate.works and candidate.mode in (AdbMode.BUNDLED, AdbMode.EXTERNAL):
        return candidate.path

    if explicit:
        raise AdbError(f"adb not found or not usable at the configured path: {explicit}")
    return None


def adb_source(explicit: Optional[str] = None):
    """Return the full :class:`~app.runtime.AdbCandidate` for reporting in the UI."""
    from .runtime import locate_adb

    return locate_adb(explicit)


class Adb:
    """A single adb installation, bound to an executable path."""

    def __init__(self, executable: Optional[str] = None, cancel_check=None):
        self.executable = find_adb(executable)
        if not self.executable:
            raise AdbError(
                "adb was not found. Install Android Platform Tools and make sure adb is on PATH, "
                "or set the ADB_PATH environment variable to the adb executable."
            )
        self._cancel_check = cancel_check

    # -- low level ---------------------------------------------------------

    def _cancelled(self) -> bool:
        return bool(self._cancel_check and self._cancel_check())

    def run(
        self,
        args: Sequence[str],
        timeout: float = DEFAULT_TIMEOUT,
        check: bool = False,
    ) -> AdbResult:
        """Run `adb <args...>`. Raises AdbError only on timeout or spawn failure."""
        if self._cancelled():
            raise AdbError("operation cancelled")
        argv = [self.executable, *args]
        try:
            proc = subprocess.run(
                argv,
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
                timeout=timeout,
            )
        except subprocess.TimeoutExpired as exc:
            return AdbResult(
                argv=list(argv),
                returncode=-1,
                stdout=(exc.stdout or "") if isinstance(exc.stdout, str) else "",
                stderr=(exc.stderr or "") if isinstance(exc.stderr, str) else "",
                timed_out=True,
            )
        except OSError as exc:
            raise AdbError(f"failed to execute {self.executable}: {exc}") from exc

        res = AdbResult(list(argv), proc.returncode, proc.stdout or "", proc.stderr or "")
        if check and not res.ok:
            raise AdbError(f"adb failed ({res.returncode}): {res.output}")
        return res

    def shell(
        self,
        serial: str,
        command: Sequence[str],
        timeout: float = DEFAULT_TIMEOUT,
        as_root: bool = False,
    ) -> AdbResult:
        """Run a device shell command.

        `adb shell` flattens its arguments into a single string that the *device's* shell then
        re-parses. That means an argument containing spaces would be split into several, and an
        argument containing shell metacharacters could be interpreted. Both are real hazards here
        because the alert body is operator-supplied. Every argument is therefore quoted with
        :func:`shlex.quote` before being joined, so the device shell sees exactly one word per
        argument and nothing we pass can be read as shell syntax.
        """
        import shlex

        remote = " ".join(shlex.quote(a) for a in command)
        args: List[str] = ["-s", serial, "shell"]
        if as_root:
            args.append("su")
            args.append("-c")
        args.append(remote)
        return self.run(args, timeout=timeout)

    def is_alive(self, serial: str) -> bool:
        return self.run(["-s", serial, "get-state"], timeout=10).stdout.strip() == "device"

    # -- discovery ---------------------------------------------------------

    def devices(self) -> List[Device]:
        """Parse `adb devices -l` into structured devices. Listing is not readiness."""
        res = self.run(["devices", "-l"], timeout=DEFAULT_TIMEOUT)
        devices: List[Device] = []
        for raw in res.stdout.splitlines():
            line = raw.strip()
            if not line or line.startswith("List of devices"):
                continue
            parts = line.split()
            if len(parts) < 2:
                continue
            serial, token = parts[0], parts[1]
            extra: Dict[str, str] = {}
            for token_part in parts[2:]:
                if ":" in token_part:
                    k, _, v = token_part.partition(":")
                    extra[k] = v

            dev = Device(serial=serial)
            if token == "device":
                # A listed `device` can still be mid-boot and reject shell commands.
                if self.is_alive(serial):
                    dev.state = DeviceState.READY
                else:
                    dev.state = DeviceState.BUSY
            elif token == "unauthorized":
                dev.state = DeviceState.UNAUTHORIZED
            elif token == "offline":
                dev.state = DeviceState.OFFLINE
            else:
                dev.state = DeviceState.UNKNOWN

            if "model" in extra:
                dev.model = extra["model"].replace("_", " ")
            if "product" in extra:
                dev.product = extra["product"]
            devices.append(dev)
        return devices

    # -- inspection --------------------------------------------------------

    def getprop(self, serial: str, prop: str, timeout: float = DEFAULT_TIMEOUT) -> str:
        res = self.run(["-s", serial, "shell", "getprop", prop], timeout=timeout)
        return res.stdout.strip()

    def is_root_shell(self, serial: str) -> bool:
        """Whether the *current* adb shell is already uid 0."""
        res = self.run(["-s", serial, "shell", "id"], timeout=15)
        return bool(re.search(r"uid=0\b", res.stdout))

    def has_su(self, serial: str) -> bool:
        res = self.run(["-s", serial, "shell", "which", "su"], timeout=15)
        return res.returncode == 0 and res.stdout.strip().endswith("su")

    def try_root(self, serial: str) -> bool:
        """Attempt `adb root` (userdebug/eng only) and fall back to `su`.

        Returns True if a uid-0 shell can be established. Never raises for a mere refusal.
        """
        if self.is_root_shell(serial):
            return True
        # adbd on a userdebug build can restart itself as root; harmless if unsupported.
        self.run(["-s", serial, "root"], timeout=30)
        # adb root drops the connection briefly.
        self.run(["-s", serial, "wait-for-device"], timeout=60)
        if self.is_root_shell(serial):
            return True
        # Rooted retail device: escalate per command via su.
        if self.has_su(serial):
            res = self.run(["-s", serial, "shell", "su", "-c", "id"], timeout=20)
            if re.search(r"uid=0\b", res.stdout):
                return True
        return False

    def wait_for_boot(self, serial: str, timeout: float = BOOT_TIMEOUT) -> bool:
        res = self.run(["-s", serial, "wait-for-device"], timeout=timeout)
        if not res.ok:
            return False
        deadline = timeout
        step = 5.0
        waited = 0.0
        while waited < deadline:
            if self._cancelled():
                return False
            booted = self.getprop(serial, "sys.boot_completed", timeout=15)
            if booted.strip() == "1":
                return True
            import time as _t

            _t.sleep(step)
            waited += step
        return False

    def find_cellbroadcast_package(self, serial: str) -> Optional[str]:
        for pkg in CELLBROADCAST_PACKAGES:
            res = self.run(["-s", serial, "shell", "pm", "path", pkg], timeout=20)
            if res.stdout.strip().startswith("package:"):
                return pkg
        # Fall back to a listing, in case an OEM renamed it.
        res = self.run(["-s", serial, "shell", "pm", "list", "packages"], timeout=30)
        for line in res.stdout.splitlines():
            name = line.strip().removeprefix("package:")
            if "cellbroadcastreceiver" in name:
                return name
        return None

    def describe(self, serial: str) -> Device:
        """Fill in a Device with properties from the live target."""
        dev = Device(serial=serial)
        if not self.is_alive(serial):
            dev.state = DeviceState.OFFLINE
            return dev
        for field_name, prop in DEVICE_PROPS.items():
            setattr(dev, field_name, self.getprop(serial, prop))
        dev.is_root = self.try_root(serial)
        dev.cellbroadcast_package = self.find_cellbroadcast_package(serial)
        return dev

    # -- device file access (root only) ------------------------------------
    #
    # Two ways to be root on a device: `adb root` makes the shell itself uid 0 (userdebug/eng), or
    # `su` escalates per command (rooted retail). The first is cheaper and more common, so it is
    # tried first; `su` is only used when the shell is not already root.

    def read_file(self, serial: str, path: str, timeout: float = 20) -> Optional[str]:
        """Read a root-only file. Returns None if it cannot be read."""
        if self.is_root_shell(serial):
            res = self.run(["-s", serial, "shell", "cat", path], timeout=timeout)
            return res.stdout if res.returncode == 0 else None
        if self.has_su(serial):
            res = self.run(["-s", serial, "shell", "su", "-c", f"cat '{path}'"], timeout=timeout)
            if res.stdout:
                return res.stdout
        # Last resort: adb pull through a world-readable staging copy.
        res = self.run(["-s", serial, "shell", "cp", path, "/data/local/tmp/_es_read"],
                       timeout=timeout)
        if res.returncode == 0:
            pulled = self.run(["-s", serial, "pull", "/data/local/tmp/_es_read", "-"],
                              timeout=timeout)
            self.run(["-s", serial, "shell", "rm", "-f", "/data/local/tmp/_es_read"], timeout=10)
            if pulled.stdout:
                return pulled.stdout
        return None

    def write_file(self, serial: str, path: str, content: str, timeout: float = 20) -> bool:
        """Write a root-only file. The path is single-quoted and content arrives on stdin."""
        if self.is_root_shell(serial):
            # The redirection must happen on the device, so the whole thing is one remote argument
            # containing only our single-quoted path -- no user input reaches it.
            argv = [self.executable, "-s", serial, "shell", f"cat > '{path}'"]
        else:
            argv = [self.executable, "-s", serial, "shell", "su", "-c", f"cat > '{path}'"]
        try:
            proc = subprocess.run(
                argv,
                input=content,
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
                timeout=timeout,
            )
        except (subprocess.TimeoutExpired, OSError):
            return False
        return proc.returncode == 0

    # Backwards-compatible aliases used elsewhere.
    def cat_as_root(self, serial: str, path: str, timeout: float = 20) -> Optional[str]:
        return self.read_file(serial, path, timeout=timeout)

    def write_as_root(self, serial: str, path: str, content: str, timeout: float = 20) -> bool:
        return self.write_file(serial, path, content, timeout=timeout)

    def push(self, serial: str, local: str, remote: str, timeout: float = 60) -> bool:
        res = self.run(["-s", serial, "push", local, remote], timeout=timeout)
        return res.returncode == 0

    def logcat_dump(self, serial: str, lines: int = 4000, timeout: float = 40) -> str:
        res = self.run(["-s", serial, "logcat", "-d", "-t", str(lines), "-v", "threadtime"],
                       timeout=timeout)
        return res.stdout

    def logcat_clear(self, serial: str, timeout: float = 20) -> bool:
        return self.run(["-s", serial, "logcat", "-c"], timeout=timeout).returncode == 0


def platform_setup_hint() -> str:
    """A short, accurate message about obtaining adb. We never download it for the user."""
    from .runtime import adb_setup_hint

    return adb_setup_hint() + f"\n\n  (platform: {sys.platform})"