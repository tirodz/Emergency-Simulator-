"""Typed models shared by the CLI, the controller and the desktop UI."""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import Enum
from typing import List, Optional


class DeviceState(str, Enum):
    """How usable a device is. 'Listed by adb devices' is not the same as READY."""

    READY = "READY"
    BUSY = "BUSY"
    OFFLINE = "OFFLINE"
    UNAUTHORIZED = "UNAUTHORIZED"
    NO_ROOT = "NO_ROOT"
    UNSUPPORTED = "UNSUPPORTED"
    UNKNOWN = "UNKNOWN"


class AlertState(str, Enum):
    """Progress of one send attempt, in order."""

    IDLE = "IDLE"
    PREPARING = "PREPARING"
    READY_TO_SEND = "READY_TO_SEND"
    SENDING = "SENDING"
    RECEIVED_BY_CELLBROADCAST = "RECEIVED_BY_CELLBROADCAST"
    ALERT_DISPLAYED = "ALERT_DISPLAYED"
    FAILED = "FAILED"
    CANCELLED = "CANCELLED"


class TransactionState(str, Enum):
    """Per-device send gate.

    Android queues emergency alerts and there is no way to withdraw one once it is displayed, so a
    second send is not a harmless retry: it leaves two dialogs stacked on the device. The controller
    therefore remembers the state of the last attempt per device and refuses a new one until the
    operator has explicitly acknowledged the previous outcome.
    """

    READY = "READY"
    BUSY = "BUSY"
    DELIVERED = "DELIVERED"
    UNCERTAIN = "UNCERTAIN"


class FailureCode(str, Enum):
    NO_DEVICE = "NO_DEVICE"
    DEVICE_OFFLINE = "DEVICE_OFFLINE"
    DEVICE_UNAUTHORIZED = "DEVICE_UNAUTHORIZED"
    DEVICE_BUSY = "DEVICE_BUSY"
    DEVICE_UNSUPPORTED = "DEVICE_UNSUPPORTED"
    NO_ADB = "NO_ADB"
    NO_ROOT = "NO_ROOT"
    CELLBROADCAST_MISSING = "CELLBROADCAST_MISSING"
    TEST_MODE_DISABLED = "TEST_MODE_DISABLED"
    INJECTOR_MISSING = "INJECTOR_MISSING"
    INJECTOR_BUILD_FAILED = "INJECTOR_BUILD_FAILED"
    INJECTOR_FAILURE = "INJECTOR_FAILURE"
    BROADCAST_REJECTED = "BROADCAST_REJECTED"
    CELLBROADCAST_FILTERED = "CELLBROADCAST_FILTERED"
    ALERT_PROCESSING_FAILED = "ALERT_PROCESSING_FAILED"
    TIMEOUT = "TIMEOUT"
    INVALID_BODY = "INVALID_BODY"
    USER_CANCELLED = "USER_CANCELLED"
    DUPLICATE_SEND_BLOCKED = "DUPLICATE_SEND_BLOCKED"
    UNKNOWN = "UNKNOWN"


@dataclass
class SupportLevel(str, Enum):
    """How confidently the tool can drive a device, based on what it observed.

    Deliberately not a single boolean. A device can be reachable by ADB yet unable to receive an
    injected test alert, and the difference matters to the operator, so the levels name the reason.
    """

    SUPPORTED = "SUPPORTED"
    ROOT_REQUIRED = "ROOT_REQUIRED"
    PARTIALLY_SUPPORTED = "PARTIALLY_SUPPORTED"
    UNTESTED = "UNTESTED"
    UNSUPPORTED = "UNSUPPORTED"


@dataclass
class Device:
    """A connected Android target and everything we can learn about it."""

    serial: str
    state: DeviceState = DeviceState.UNKNOWN
    model: str = ""
    product: str = ""
    release: str = ""
    sdk: str = ""
    build_type: str = ""
    debuggable: str = ""
    is_root: bool = False
    cellbroadcast_package: Optional[str] = None
    notes: List[str] = field(default_factory=list)

    # -- observed capability ------------------------------------------------
    # Each is a separate fact, recorded because the operator needs to know which specific step
    # failed rather than a single opaque "not compatible".
    adb_connected: bool = False
    build_fingerprint: str = ""
    one_ui_version: str = ""
    manufacturer: str = ""
    test_mode_ready: bool = False

    @property
    def support_level(self) -> "SupportLevel":
        """Classify this device from what was actually observed, never from its brand."""
        if self.state is DeviceState.UNSUPPORTED:
            return SupportLevel.UNSUPPORTED
        if not self.adb_connected:
            return SupportLevel.UNTESTED
        if self.cellbroadcast_package is None:
            return SupportLevel.UNSUPPORTED
        if self.state is DeviceState.READY and self.is_root and self.cellbroadcast_package:
            return SupportLevel.SUPPORTED
        # Everything else that could be made to work with a privilege or a configuration change.
        return SupportLevel.ROOT_REQUIRED

    @property
    def support_reason(self) -> str:
        """Plain-language explanation for the UI, matching :attr:`support_level`."""
        level = self.support_level
        if level is SupportLevel.SUPPORTED:
            return "This device meets every requirement for a test alert."
        if level is SupportLevel.ROOT_REQUIRED:
            return (
                "This device has the Cell Broadcast subsystem but does not provide the "
                "system-level injection authority required in its current state."
            )
        if level is SupportLevel.UNSUPPORTED:
            if self.cellbroadcast_package is None:
                return "No Cell Broadcast receiver package was found on this device."
            return "This device cannot be used for a test alert."
        return "This device has not been tested."

    @property
    def label(self) -> str:
        bits = [self.serial]
        if self.release:
            bits.append(f"Android {self.release}")
        if self.sdk:
            bits.append(f"API {self.sdk}")
        return "  ".join(bits)

    @property
    def is_usable(self) -> bool:
        return (
            self.state is DeviceState.READY
            and self.is_root
            and self.cellbroadcast_package is not None
        )


@dataclass
class TestModeStatus:
    """Whether the receiving app will accept an ETWS test alert."""

    testing_mode: bool = False
    enable_test_alerts: bool = False
    prefs_path: Optional[str] = None
    changed: bool = False
    method: str = ""
    error: Optional[str] = None

    @property
    def satisfied(self) -> bool:
        return self.testing_mode and self.enable_test_alerts


@dataclass
class SendResult:
    """The outcome of one send attempt, backed by downstream evidence."""

    state: AlertState = AlertState.IDLE
    failure: Optional[FailureCode] = None
    message: str = ""
    device_serial: str = ""
    body: str = ""
    category: int = 0
    injector_exit_code: Optional[int] = None
    injector_stdout: str = ""
    injector_stderr: str = ""
    evidence: List[str] = field(default_factory=list)

    @property
    def ok(self) -> bool:
        return self.state is AlertState.ALERT_DISPLAYED