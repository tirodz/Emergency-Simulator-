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
    UNKNOWN = "UNKNOWN"


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