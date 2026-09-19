"""The control engine: device preparation, injection, and evidence-based result detection.

This module knows nothing about the UI or the CLI. Both drive it the same way, which is what keeps
the Tkinter front end and the command line honest about doing the same thing.

Safety properties enforced here, not merely documented:

* The service category is a module constant. There is no parameter, config key or API field that
  can change it, so no caller can reach a real hazard class.
* A body that does not begin with ``TEST`` is rejected before anything touches the device.
* Sending is always a separate, explicitly-requested step. :meth:`send_test_alert` never runs on its
  own; it is called only after the caller obtained confirmation.
* Nothing here can transmit. There is no modem, radio or RF code path, and no on-device command in
  this module touches CbConfig or the telephony stack.
"""

from __future__ import annotations

import logging
import os
import platform
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path
from typing import Callable, List, Optional

from .adb import Adb, AdbError, platform_setup_hint
from .runtime import INJECTOR_JAR_NAME, runtime_layout
from .models import (
    AlertState,
    Device,
    DeviceState,
    FailureCode,
    SendResult,
    TestModeStatus,
    TransactionState,
)

log = logging.getLogger("emergency_simulator")

# ---------------------------------------------------------------------------
# Safety-critical constants. These are NOT configurable, by design.
# ---------------------------------------------------------------------------

#: ETWS test channel. 0x1103 == 4355, from AOSP res/values/config.xml
#: (etws_test_alerts_range_strings). This is the only category the tool can send.
SERVICE_CATEGORY = 4355

#: The secret code that toggles CellBroadcast testing mode. A TOGGLE, not a setter.
TESTING_MODE_SECRET_CODE = "android_secret_code://2627"
TESTING_MODE_ACTION = "android.telephony.action.SECRET_CODE"

#: Mandatory body prefix. An alert that is not obviously a test must never be sent.
REQUIRED_PREFIX = "TEST"
DEFAULT_BODY = "TEST ALERT - SIMULATION"

#: The injector jar is located through the runtime layout, which knows about both a repository
#: checkout and a packaged build (where resources live in PyInstaller's unpack directory). The
#: release ships the jar prebuilt, so a user never needs the Android SDK or a JDK.
_INJECTOR_CACHE: dict = {}


def injector_jar_path() -> Optional[Path]:
    """Path to the prebuilt injector jar, or None when it is genuinely absent."""
    layout = runtime_layout()
    found = layout.injector_jar
    _INJECTOR_CACHE["jar"] = found
    _INJECTOR_CACHE["layout"] = layout
    return found


INJECTOR_REMOTE = "/data/local/tmp/alertinject.jar"
INJECTOR_CLASS = "org.emergencysim.alertinject.AlertInjector"

#: Logcat markers proving the genuine pipeline handled the message. Order matters: the first is the
#: earliest evidence and the last is the strongest.
EVIDENCE_MARKERS = (
    ("CellBroadcastReceiver", re.compile(r"CellBroadcastReceiver.*onReceive")),
    ("CBAlertService", re.compile(r"CBAlertService.*onStartCommand")),
    ("CellBroadcastAlertAudio", re.compile(r"CellBroadcastAlertAudio")),
    ("CellBroadcastAlertDialog", re.compile(r"CellBroadcastAlertDialog")),
)

#: Logcat lines that mean the message was rejected or dropped after reaching the receiver.
FILTER_MARKERS = (
    ("filtered by user preference", re.compile(r"ignoring alert of type .* by user preference")),
    ("undefined channel", re.compile(r"received undefined channels")),
    ("no extras", re.compile(r"received SMS_CB_RECEIVED_ACTION with no extras")),
    ("permission denial", re.compile(r"Permission Denial: not allowed to send broadcast")),
)

#: How long to wait for the alert to appear after the injector exits.
EVIDENCE_TIMEOUT = 45.0


class Cancelled(RuntimeError):
    """Raised when the caller asked to stop a pending operation."""


class SafetyError(ValueError):
    """Raised when a caller tries to do something the tool refuses to do."""


def validate_body(body: str) -> str:
    """Normalise and validate a test alert body.

    Returns the body to use, which always begins with ``TEST``. Raises :class:`SafetyError`
    otherwise. An empty body becomes the default rather than being sent.
    """
    body = (body or "").strip()
    if not body:
        return DEFAULT_BODY
    body = " ".join(body.split())
    if not body.startswith(REQUIRED_PREFIX):
        raise SafetyError(
            f"refusing to send: the message must begin with {REQUIRED_PREFIX!r} so the alert is "
            f"unambiguously identifiable as a test. Got: {body!r}"
        )
    if len(body) > 300:
        raise SafetyError("refusing to send: the message is longer than 300 characters")
    return body


class EmergencySimulatorController:
    """Drives one rooted Android target through the proven Cell Broadcast test path."""

    def __init__(
        self,
        adb_path: Optional[str] = None,
        cancel_check: Optional[Callable[[], bool]] = None,
        on_log: Optional[Callable[[str], None]] = None,
    ):
        self.on_log = on_log or (lambda _m: None)
        self._cancel_check = cancel_check or (lambda: False)
        self._adb: Optional[Adb] = None
        self._adb_path = adb_path
        self.adb_error: Optional[str] = None
        # Per-device send gate, keyed by serial. See :class:`~app.models.TransactionState`.
        self._transactions: dict = {}
        try:
            self._adb = Adb(adb_path, cancel_check=cancel_check)
        except AdbError as exc:
            self.adb_error = str(exc)

    # -- transaction gate --------------------------------------------------

    def transaction_state(self, serial: str) -> TransactionState:
        return self._transactions.get(serial, TransactionState.READY)

    def acknowledge(self, serial: str) -> None:
        """Clear the gate after the operator has dealt with the device.

        Deliberately explicit. The controller will not decide on the operator's behalf that a
        displayed alert has been dismissed, because it has no way to know.
        """
        self._transactions[serial] = TransactionState.READY

    def _gate(self, serial: str) -> Optional[str]:
        """Return a refusal reason if this device may not be sent to yet."""
        state = self.transaction_state(serial)
        if state is TransactionState.READY:
            return None
        return self.gate_explanation(serial)

    def gate_explanation(self, serial: str) -> str:
        """Why this device is currently gated, in operator-facing terms."""
        state = self.transaction_state(serial)
        if state is TransactionState.BUSY:
            return "a test alert is already being sent to this device"
        if state is TransactionState.DELIVERED:
            return (
                "the previous test alert is still outstanding on this device. Android queues "
                "alerts, so sending again would stack a second dialog. Dismiss the alert on the "
                "device, then acknowledge it here."
            )
        if state is TransactionState.UNCERTAIN:
            return (
                "the previous attempt to this device had an uncertain outcome. Check the device "
                "screen before sending again: a timeout is not proof that the alert was not "
                "delivered."
            )
        return ""

    # -- logging -----------------------------------------------------------

    def _say(self, message: str, level: int = logging.INFO) -> None:
        log.log(level, message)
        try:
            self.on_log(message)
        except Exception:  # a broken UI callback must never break the engine
            pass

    # -- properties --------------------------------------------------------

    @property
    def adb(self) -> Adb:
        if self._adb is None:
            raise AdbError(self.adb_error or platform_setup_hint())
        return self._adb

    @property
    def cancel_requested(self) -> bool:
        return bool(self._cancel_check())

    def _check_cancel(self) -> None:
        if self.cancel_requested:
            raise Cancelled("cancelled by operator")

    # -- discovery ---------------------------------------------------------

    def discover(self) -> List[Device]:
        """List attached devices with their readiness assessed, not merely enumerated."""
        if self._adb is None:
            raise AdbError(self.adb_error or platform_setup_hint())
        self._say("Querying adb for attached devices")
        devices = self.adb.devices()
        for dev in devices:
            if dev.state in (DeviceState.OFFLINE, DeviceState.UNAUTHORIZED, DeviceState.BUSY):
                continue
            self._inspect(dev)
        usable = [d for d in devices if d.is_usable]
        self._say(f"Found {len(devices)} device(s); {len(usable)} ready for test alerts")
        return devices

    def _inspect(self, dev: Device) -> None:
        """Fill in properties and assess readiness for one device."""
        for field_name, prop in (
            ("model", "ro.product.model"),
            ("product", "ro.product.device"),
            ("release", "ro.build.version.release"),
            ("sdk", "ro.build.version.sdk"),
            ("build_type", "ro.build.type"),
            ("debuggable", "ro.debuggable"),
        ):
            setattr(dev, field_name, self.adb.getprop(dev.serial, prop))

        dev.adb_connected = True
        # Read for diagnostics only. Nothing in the send path depends on the brand, and OEM identity
        # deliberately does not influence the support verdict: that comes from observed capability.
        dev.build_fingerprint = self.adb.getprop(dev.serial, "ro.build.fingerprint")
        dev.manufacturer = self.adb.getprop(dev.serial, "ro.product.manufacturer")
        one_ui = self.adb.getprop(dev.serial, "ro.build.version.oneui")
        if one_ui:
            dev.one_ui_version = one_ui

        if not dev.is_root:
            dev.is_root = self.adb.try_root(dev.serial)

        dev.cellbroadcast_package = self.adb.find_cellbroadcast_package(dev.serial)

        if dev.cellbroadcast_package is None:
            dev.state = DeviceState.UNSUPPORTED
            dev.notes.append("no CellBroadcast receiver package found")
        elif not dev.is_root:
            dev.state = DeviceState.NO_ROOT
            dev.notes.append("root unavailable; the injected broadcast is a protected broadcast")
        else:
            dev.state = DeviceState.READY

    def select_device(self, serial: Optional[str]) -> Device:
        """Return one device, preferring the given serial. Raises if none is usable."""
        devices = self.discover()
        if not devices:
            raise SafetyError("NO_DEVICE")
        if serial:
            for d in devices:
                if d.serial == serial:
                    return d
            raise SafetyError(f"device not found: {serial}")
        usable = [d for d in devices if d.is_usable]
        if not usable:
            # Surface the most informative reason rather than a generic failure.
            for d in devices:
                if d.state is DeviceState.UNAUTHORIZED:
                    return d
            return devices[0]
        return usable[0]

    def describe_device(self, dev: Device) -> str:
        return (
            f"Device: {dev.serial}\n"
            f"Model: {dev.model or 'unknown'}\n"
            f"Android: {dev.release or '?'} / API {dev.sdk or '?'}\n"
            f"Build: {dev.build_type or '?'} (debuggable={dev.debuggable or '?'})\n"
            f"Root: {'YES' if dev.is_root else 'NO'}\n"
            f"CellBroadcast: {dev.cellbroadcast_package or 'NOT FOUND'}"
        )

    # -- test mode ---------------------------------------------------------

    def _prefs_path(self, dev: Device) -> str:
        pkg = dev.cellbroadcast_package or "com.google.android.cellbroadcastreceiver"
        return (
            f"/data/user_de/0/{pkg}/shared_prefs/{pkg}_preferences.xml"
        )

    def read_test_mode(self, dev: Device) -> TestModeStatus:
        """Read the current test-alert prerequisites from the receiver's own preferences."""
        status = TestModeStatus()
        if not dev.is_root:
            status.error = "root required to read the receiver's preferences"
            return status
        path = self._prefs_path(dev)
        status.prefs_path = path
        content = self.adb.read_file(dev.serial, path)
        if content is None:
            exists = self.adb.run(["-s", dev.serial, "shell", "ls", path], timeout=15)
            if exists.returncode != 0:
                status.error = f"preferences file does not exist yet: {path}"
            else:
                status.error = f"could not read {path} (root available but read failed)"
            return status

        def flag(name: str) -> bool:
            m = re.search(rf'name="{name}"\s+value="([^"]*)"', content)
            return bool(m) and m.group(1).lower() == "true"

        status.testing_mode = flag("testing_mode")
        status.enable_test_alerts = flag("enable_test_alerts")
        return status

    def prepare_test_mode(self, dev: Device, dry_run: bool = False) -> TestModeStatus:
        """Establish ``testing_mode`` and ``enable_test_alerts``.

        The secret code is tried first because it is the vendor's own mechanism, but it is a
        *toggle*: we read the current state and only send it when the state is off. Whatever the
        secret code leaves unset is then written directly into the receiver's preference file, which
        is the documented fallback. The receiver is force-stopped afterwards because it caches the
        values in memory.
        """
        self._check_cancel()
        status = self.read_test_mode(dev)
        prefs_missing = bool(status.error and "does not exist yet" in status.error)
        if status.error and not prefs_missing:
            # A read failure is not the same as "disabled". Refuse rather than guess, because
            # guessing wrong on a toggle-style setting could disable a working configuration.
            raise SafetyError(
                f"cannot read the CellBroadcast test configuration: {status.error}. "
                "Refusing to change device state blindly."
            )

        if status.satisfied:
            status.method = "already enabled"
            self._say("Test mode already enabled; leaving device configuration untouched")
            return status

        if dry_run:
            need = []
            if not status.testing_mode:
                need.append("testing_mode")
            if not status.enable_test_alerts:
                need.append("enable_test_alerts")
            status.method = f"would enable: {', '.join(need)}"
            self._say(f"[dry-run] would enable {', '.join(need)}")
            return status

        # 1. testing_mode, via the vendor secret code -- only if currently off, because it toggles.
        if not status.testing_mode:
            self._say("Enabling testing mode via the CellBroadcast secret code")
            res = self.adb.run(
                [
                    "-s", dev.serial, "shell", "am", "broadcast",
                    "-a", TESTING_MODE_ACTION,
                    "-d", TESTING_MODE_SECRET_CODE,
                ],
                timeout=30,
            )
            if not res.ok:
                self._say(f"Secret-code broadcast failed: {res.output}", logging.WARNING)
            read_back = self.read_test_mode(dev)
            if read_back.testing_mode:
                status.testing_mode = True
                status.method = "secret code"
                self._say("Testing mode is now enabled")
            else:
                self._say(
                    "Secret code did not enable testing mode (it is a toggle, and may have been "
                    "out of step); falling back to the preference file",
                    logging.WARNING,
                )

        # 2. enable_test_alerts (and testing_mode, if the secret code did not take) via the prefs
        #    file. This is the documented fallback and the only way to set enable_test_alerts.
        if not status.satisfied:
            self._write_prefs(dev, status)

        status = self.read_test_mode(dev)
        if status.satisfied:
            self._say(
                f"Test mode satisfied via {status.method or 'preference file'}: "
                "testing_mode=true, enable_test_alerts=true"
            )
        else:
            self._say("Test mode could NOT be established", logging.ERROR)
        return status

    def _write_prefs(self, dev: Device, status: TestModeStatus) -> None:
        path = status.prefs_path or self._prefs_path(dev)
        content = self.adb.cat_as_root(dev.serial, path)
        if content is None:
            # No file yet: create a minimal one. Use the device's own primary user dir.
            content = '<?xml version=\'1.0\' encoding=\'utf-8\' standalone=\'yes\' ?>\n<map>\n</map>\n'
            self._say("Receiver preferences file absent; creating a minimal one")

        def set_flag(xml: str, name: str, value: str) -> str:
            pattern = rf'(<boolean\s+name="{name}"\s+value=")[^"]*("\s*/>)'
            if re.search(pattern, xml):
                return re.sub(pattern, rf"\g<1>{value}\g<2>", xml)
            return xml.replace("</map>", f'    <boolean name="{name}" value="{value}" />\n</map>')

        updated = content
        if not status.enable_test_alerts:
            updated = set_flag(updated, "enable_test_alerts", "true")
        if not status.testing_mode:
            updated = set_flag(updated, "testing_mode", "true")

        if updated == content:
            return
        self._say(f"Writing test-alert preferences to {path}")
        if not self.adb.write_as_root(dev.serial, path, updated):
            raise SafetyError(
                f"could not write {path}. The device is rooted but the write failed; refusing to "
                "continue because the alert would be silently filtered."
            )
        status.changed = True
        if not status.method:
            status.method = "preference file"

        # The receiver caches these in memory, so it must be restarted.
        pkg = dev.cellbroadcast_package or "com.google.android.cellbroadcastreceiver"
        self._say(f"Restarting {pkg} so it re-reads its preferences")
        self.adb.run(["-s", dev.serial, "shell", "am", "force-stop", pkg], timeout=20)
        time.sleep(2)

    # -- injector ----------------------------------------------------------

    def ensure_injector_built(self, dry_run: bool = False) -> Path:
        """Resolve the injector jar, building it only in a development checkout.

        A release ships the jar prebuilt and must never need the Android SDK. In a checkout we may
        rebuild it, but only if the sources are actually present and newer than the jar; otherwise a
        stale jar silently masks source changes.
        """
        layout = runtime_layout()
        jar = layout.injector_jar
        frozen = layout.is_frozen

        if jar is None:
            if frozen:
                raise SafetyError(
                    "the bundled Android injector is missing from this build.\n"
                    "  This is a packaging fault, not a device fault: the release archive should\n"
                    "  contain android/alertinject/out/alertinject.jar.\n"
                    "  Development runs can rebuild it with android/alertinject/build.sh."
                )
            raise SafetyError(
                "the Android injector jar is missing and no sources were found to build it.\n"
                "  Expected the jar at android/alertinject/out/alertinject.jar."
            )

        # A release is self-contained by definition; never try to rebuild it.
        if frozen:
            self._say(f"Using bundled injector: {jar.name}")
            return jar

        sources = sorted(layout.injector_jar.parent.parent.rglob("*.java"))
        if not sources:
            self._say(f"Using prebuilt injector: {jar.name}")
            return jar

        jar_mtime = jar.stat().st_mtime
        stale = any(s.stat().st_mtime > jar_mtime for s in sources)
        if not stale:
            self._say(f"Injector jar is current: {jar.name}")
            return jar

        if dry_run:
            self._say(f"[dry-run] would rebuild the injector from {len(sources)} source file(s)")
            return jar

        script = jar.parent.parent / ("build.ps1" if os.name == "nt" else "build.sh")
        if os.name == "nt" and not script.exists():
            raise SafetyError(
                "building the injector on Windows needs an Android SDK plus JDK; the repository "
                "ships build.sh. Build the jar on a Unix-like host, or set ANDROID_HOME and JAVA_HOME "
                "and run it under WSL/Git Bash, then re-run this tool."
            )
        self._say(f"Building the injector with {script.name}")
        env = dict(os.environ)
        try:
            proc = subprocess.run(
                ["bash", str(script)],
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
                timeout=300,
                cwd=str(script.parent),
                env=env,
            )
        except (subprocess.TimeoutExpired, OSError) as exc:
            raise SafetyError(f"injector build failed to run: {exc}") from exc

        if proc.returncode != 0 or not jar.exists():
            tail = (proc.stdout + proc.stderr).strip().splitlines()[-8:]
            raise SafetyError("injector build failed:\n" + "\n".join(tail))
        self._say(f"Built {jar}")
        return jar

    # -- send --------------------------------------------------------------

    def send_test_alert(
        self,
        dev: Device,
        body: str = DEFAULT_BODY,
        dry_run: bool = False,
    ) -> SendResult:
        """Deliver one ETWS test alert and determine the outcome from device evidence.

        The service category is fixed. `body` is validated here as well as by the caller, so no path
        into this method can send a non-test message.
        """
        result = SendResult(device_serial=dev.serial, category=SERVICE_CATEGORY)
        try:
            body = validate_body(body)
        except SafetyError as exc:
            result.state = AlertState.FAILED
            result.failure = FailureCode.INVALID_BODY
            result.message = str(exc)
            self._say(result.message, logging.ERROR)
            return result
        result.body = body

        # The gate is checked before anything else, including in a dry run, so the operator learns
        # about an outstanding alert without us touching the device at all.
        refusal = self._gate(dev.serial)
        if refusal:
            result.state = AlertState.FAILED
            result.failure = FailureCode.DUPLICATE_SEND_BLOCKED
            result.message = refusal
            self._say(f"refusing to send: {refusal}", logging.WARNING)
            return result

        # Refuse outright rather than half-running on an unusable device.
        if dev.state in (DeviceState.OFFLINE, DeviceState.UNKNOWN):
            result.state = AlertState.FAILED
            result.failure = FailureCode.DEVICE_OFFLINE
            result.message = f"device {dev.serial} is offline"
            return result
        if dev.state is DeviceState.UNAUTHORIZED:
            result.state = AlertState.FAILED
            result.failure = FailureCode.DEVICE_UNAUTHORIZED
            result.message = "device is unauthorized; accept the USB debugging prompt first"
            return result
        if dev.state is DeviceState.BUSY:
            result.state = AlertState.FAILED
            result.failure = FailureCode.DEVICE_BUSY
            result.message = "device is busy (still booting?)"
            return result
        if dev.cellbroadcast_package is None:
            result.state = AlertState.FAILED
            result.failure = FailureCode.CELLBROADCAST_MISSING
            result.message = "no CellBroadcast receiver package on this device"
            return result
        if not dev.is_root:
            result.state = AlertState.FAILED
            result.failure = FailureCode.NO_ROOT
            result.message = (
                "root is required: the emergency broadcast is a protected broadcast and a "
                "non-root sender is rejected by the framework"
            )
            self._say(result.message, logging.ERROR)
            return result

        try:
            result.state = AlertState.PREPARING

            # Ensure the injector exists before we touch test mode, so a build failure does not
            # leave the device reconfigured for nothing.
            jar = self.ensure_injector_built(dry_run=dry_run)
            self._check_cancel()

            mode = self.prepare_test_mode(dev, dry_run=dry_run)
            if not dry_run and not mode.satisfied:
                result.state = AlertState.FAILED
                result.failure = FailureCode.TEST_MODE_DISABLED
                result.message = (
                    "test alerts are disabled on the device and could not be enabled. The message "
                    "would be dropped as 'ignoring alert by user preference'."
                )
                return result

            if dry_run:
                self._say("[dry-run] would push and execute the injector; nothing was sent")
                result.state = AlertState.READY_TO_SEND
                result.message = "dry run: every check passed, nothing was sent"
                return result

            result.state = AlertState.READY_TO_SEND
            self._check_cancel()

            # From here on an alert may reach the device, so the gate closes. If anything goes
            # wrong after this point the outcome is uncertain rather than known-failed, and the
            # gate stays closed until the operator acknowledges.
            self._transactions[dev.serial] = TransactionState.BUSY

            # Baseline logcat so evidence cannot come from a previous run.
            self.adb.logcat_clear(dev.serial)

            if not self.adb.push(dev.serial, str(jar), INJECTOR_REMOTE):
                result.state = AlertState.FAILED
                result.failure = FailureCode.INJECTOR_FAILURE
                result.message = "failed to push the injector to the device"
                # Nothing was injected, so the device is not left in an unknown state.
                self._transactions[dev.serial] = TransactionState.READY
                return result
            self._say(f"Pushed injector to {INJECTOR_REMOTE}")

            result.state = AlertState.SENDING
            self._say(f"Sending ETWS TEST alert (category {SERVICE_CATEGORY})")
            res = self.adb.shell(
                dev.serial,
                [
                    f"CLASSPATH={INJECTOR_REMOTE}",
                    "app_process",
                    "/system/bin",
                    INJECTOR_CLASS,
                    str(SERVICE_CATEGORY),
                    body,
                ],
                timeout=60,
                as_root=not self.adb.is_root_shell(dev.serial),
            )
            result.injector_exit_code = res.returncode
            result.injector_stdout = res.stdout
            result.injector_stderr = res.stderr
            if res.timed_out:
                result.failure = FailureCode.TIMEOUT
            for line in res.output.splitlines()[:10]:
                self._say(f"  injector: {line}")

            # The injector exiting cleanly is not success. Look for the production components.
            return self._collect_evidence(dev, result)

        except Cancelled as exc:
            result.state = AlertState.CANCELLED
            result.failure = FailureCode.USER_CANCELLED
            result.message = str(exc)
            # Cancellation is only reachable before injection begins, so nothing was delivered.
            self._transactions[dev.serial] = TransactionState.READY
            self._say("Operation cancelled before delivery", logging.WARNING)
            return result
        except SafetyError as exc:
            result.state = AlertState.FAILED
            result.failure = FailureCode.INJECTOR_BUILD_FAILED
            result.message = str(exc)
            self._say(str(exc), logging.ERROR)
            return result
        except AdbError as exc:
            result.state = AlertState.FAILED
            result.failure = FailureCode.UNKNOWN
            result.message = str(exc)
            # The injector may already have run; we genuinely do not know.
            self._transactions[dev.serial] = TransactionState.UNCERTAIN
            self._say(str(exc), logging.ERROR)
            return result

    def _collect_evidence(self, dev: Device, result: SendResult) -> SendResult:
        """Poll logcat until the genuine pipeline shows what happened."""
        self._say("Waiting for downstream CellBroadcast evidence")
        deadline = time.monotonic() + EVIDENCE_TIMEOUT
        seen_dialog = False
        seen_receiver = False
        seen_service = False
        seen_audio = False
        filtered: Optional[str] = None

        while time.monotonic() < deadline:
            if self.cancel_requested:
                self._say("Stop requested while waiting for evidence", logging.WARNING)
                break
            time.sleep(2.0)
            dump = self.adb.logcat_dump(dev.serial, lines=3000, timeout=30)
            if not dump:
                continue

            for name, pattern in EVIDENCE_MARKERS:
                if pattern.search(dump):
                    if name == "CellBroadcastReceiver" and not seen_receiver:
                        seen_receiver = True
                        result.evidence.append(
                            "CellBroadcastReceiver.onReceive -- message accepted by the receiver"
                        )
                    elif name == "CBAlertService" and not seen_service:
                        seen_service = True
                        result.evidence.append(
                            "CellBroadcastAlertService.onStartCommand -- alert service started"
                        )
                    elif name == "CellBroadcastAlertAudio" and not seen_audio:
                        seen_audio = True
                        result.evidence.append(
                            "CellBroadcastAlertAudio -- genuine alert audio engaged"
                        )
                    elif name == "CellBroadcastAlertDialog" and not seen_dialog:
                        seen_dialog = True
                        result.evidence.append(
                            "CellBroadcastAlertDialog -- genuine full-screen alert displayed"
                        )

            if filtered is None:
                for label, pattern in FILTER_MARKERS:
                    m = pattern.search(dump)
                    if m:
                        filtered = label
                        break

            if seen_dialog and seen_audio:
                break
            if filtered:
                break

        if seen_dialog:
            result.state = AlertState.ALERT_DISPLAYED
            result.message = "genuine Android emergency-alert UI was displayed on the device"
            # An alert is on screen and only the operator can dismiss it.
            self._transactions[dev.serial] = TransactionState.DELIVERED
        elif filtered:
            result.state = AlertState.FAILED
            result.message = f"the message was rejected downstream: {filtered}"
            if filtered == "permission denial":
                result.failure = FailureCode.BROADCAST_REJECTED
            elif filtered == "no extras":
                result.failure = FailureCode.INJECTOR_FAILURE
            else:
                result.failure = FailureCode.CELLBROADCAST_FILTERED
            # The pipeline explicitly rejected it, so nothing is outstanding on the device.
            self._transactions[dev.serial] = TransactionState.READY
        elif seen_receiver and not seen_service:
            result.state = AlertState.FAILED
            result.failure = FailureCode.ALERT_PROCESSING_FAILED
            result.message = "the receiver saw the message but the alert service did not run"
            self._transactions[dev.serial] = TransactionState.UNCERTAIN
        elif seen_service:
            result.state = AlertState.RECEIVED_BY_CELLBROADCAST
            result.failure = FailureCode.TIMEOUT
            result.message = (
                "the alert service started but no alert UI was observed within the timeout"
            )
            self._transactions[dev.serial] = TransactionState.UNCERTAIN
        else:
            result.state = AlertState.FAILED
            result.failure = FailureCode.TIMEOUT
            result.message = (
                "no evidence of the message in the Cell Broadcast pipeline. The injector exited "
                f"with code {result.injector_exit_code}, but that alone proves nothing."
            )
            self._transactions[dev.serial] = TransactionState.UNCERTAIN
        for line in result.evidence:
            self._say(f"  evidence: {line}")
        self._say(f"Result: {result.state.value}" + (f" / {result.failure.value}" if result.failure else ""))
        return result

    # -- cancel ------------------------------------------------------------

    @staticmethod
    def cancel_explanation() -> str:
        """The honest CANCEL story, matching what was established experimentally."""
        return (
            "CANCEL stops a pending operation on this computer: nothing has been sent to the\n"
            "device yet, so nothing happens. Once an alert has been delivered, Android does not\n"
            "permit remote dismissal -- BACK is swallowed by the alert window and\n"
            "CLOSE_SYSTEM_DIALOGS is ignored. Dismiss it using the alert's own on-device control."
        )


def default_log_dir() -> Path:
    """Where the persistent log lives.

    A packaged executable is read-only relative to its own directory in the general case, so the log
    goes to a per-user location. Running from the repository keeps logs/ alongside the code.
    """
    from .runtime import runtime_layout

    layout = runtime_layout()
    if layout.is_frozen:
        import os as _os

        base = _os.environ.get("LOCALAPPDATA") or _os.environ.get("XDG_STATE_HOME")
        if base:
            return Path(base) / "Emergency-Simulator" / "logs"
        return Path.home() / ".emergency-simulator" / "logs"
    return layout.search_roots[-1] / "logs"


def settings_path() -> Path:
    """Where user settings live. Never inside the bundle: a release may be read-only."""
    from .runtime import persistent_dir

    return persistent_dir() / "settings.json"


def history_path() -> Path:
    """Where the local operation history lives."""
    from .runtime import persistent_dir

    return persistent_dir() / "history.json"