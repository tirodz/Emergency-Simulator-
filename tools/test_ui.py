#!/usr/bin/env python3
"""Headless functional tests for the Tkinter interface.

These build the real widget tree and drive the real callbacks -- no mocks of the UI. They skip
cleanly when no display is available (or when run under Xvfb, which is how CI exercises them).

    xvfb-run -a python3 tools/test_ui.py
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

try:
    import tkinter as tk
except ImportError as exc:  # tkinter genuinely absent
    print(f"SKIP: tkinter unavailable ({exc})")
    raise SystemExit(0)

FAILURES = []


def check(name: str, condition: bool, detail: str = "") -> None:
    if condition:
        print(f"  ok    {name}")
    else:
        print(f"  FAIL  {name} {detail}")
        FAILURES.append(name)


def make_root() -> "tk.Tk":
    root = tk.Tk()
    root.withdraw()
    return root


def pump(root: "tk.Tk", seconds: float = 0.4) -> None:
    """Run the Tk event loop for a while so queued work and after() callbacks fire."""
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        root.update()
        time.sleep(0.02)


def make_device(**kw):
    """A device that would be usable, so tests can vary one field at a time."""
    from app.models import Device, DeviceState

    base = dict(
        serial="emulator-5554",
        state=DeviceState.READY,
        model="sdk_gphone64_x86_64",
        release="15",
        sdk="35",
        build_type="userdebug",
        is_root=True,
        cellbroadcast_package="com.google.android.cellbroadcastreceiver",
        adb_connected=True,
    )
    base.update(kw)
    return Device(**base)


def all_text(widget) -> str:
    """Every `text` option in the widget tree, for scanning the whole surface."""
    parts = []
    try:
        parts.append(str(widget.cget("text")))
    except tk.TclError:
        pass
    for child in widget.winfo_children():
        parts.append(all_text(child))
    return " ".join(parts)


def main() -> int:
    try:
        root = make_root()
    except tk.TclError as exc:
        print(f"SKIP: no display available ({exc})")
        return 0

    from app.controller import (
        SERVICE_CATEGORY,
        EmergencySimulatorController,
        SafetyError,
        validate_body,
    )
    from app.models import AlertState, DeviceState, FailureCode, SendResult, SupportLevel
    from app.models import TransactionState
    from app.ui import SAFETY_STRIP, EmergencySimulatorUI
    from app.widgets import ERR, OK

    # A controller that never touches a device: adb is pointed at nothing and discovery is stubbed.
    controller = EmergencySimulatorController.__new__(EmergencySimulatorController)
    controller.on_log = lambda _m: None
    controller._cancel_check = lambda: False
    controller._adb = None
    controller._adb_path = None
    controller.adb_error = "test mode"
    controller._transactions = {}

    ui = EmergencySimulatorUI(root, controller=controller)
    pump(root, 0.2)

    print("title and safety strip")
    check("window title names the simulator", "EMERGENCY-SIMULATOR" in root.title())
    check("window title carries a version", "[" in root.title())
    check("safety strip states no cellular transmission",
          "NO CELLULAR TRANSMISSION" in SAFETY_STRIP)
    check("the safety strip is shown in the window",
          "NO CELLULAR TRANSMISSION" in ui.safety_banner._label.cget("text"))

    print("destructive action is disabled until valid")
    check("SEND starts disabled", not ui.btn_send._enabled)
    check("STOP starts disabled", not ui.btn_stop._enabled)
    check("acknowledge starts disabled", not ui.btn_ack._enabled)

    print("message validation through the UI")
    ui.msg_var.set("EXTREME THREAT - TORNADO WARNING")
    pump(root, 0.05)
    try:
        validate_body(ui.msg_var.get())
        check("hazard wording rejected", False)
    except SafetyError:
        check("hazard wording rejected", True)
    feedback = ui.msg_feedback.cget("text")
    check("rejection is shown to the operator",
          "rejected" in feedback.lower() or "must begin" in feedback.lower())
    check("rejection is coloured as an error", ui.msg_feedback.cget("fg") == ERR)

    ui.msg_var.set("TEST DRILL - HOUSEHOLD DEVICE")
    pump(root, 0.05)
    check("test wording accepted",
          validate_body(ui.msg_var.get()) == "TEST DRILL - HOUSEHOLD DEVICE")
    check("acceptance is confirmed to the operator",
          "accepted" in ui.msg_feedback.cget("text"))
    check("acceptance is coloured as ok", ui.msg_feedback.cget("fg") == OK)
    check("body prefix enforced in the entry widget",
          validate_body(ui.msg_var.get()).startswith("TEST"))

    print("device rendering and selection")
    devices = [
        make_device(serial="emulator-5554", model="sdk_gphone64_x86_64"),
        make_device(serial="R58M12345", model="SM-S911B", release="14", sdk="34",
                    is_root=False, state=DeviceState.NO_ROOT),
    ]
    ui._render_devices(devices)
    pump(root, 0.1)

    check("a row exists per device", len(ui._rows) == 2)
    check("a usable device is selected automatically", ui._selected is not None)
    check("selection is a usable device", ui._selected.serial == "emulator-5554")
    check("SEND is enabled for a usable device", ui.btn_send._enabled)
    check("the target panel names the device", "emulator-5554" in ui.target_value.cget("text"))
    check("the verdict is shown as a word", "SUPPORTED" in ui.target_verdict.cget("text"))

    ui._on_device_selected("R58M12345")
    pump(root, 0.05)
    check("selecting a non-root device moves the target", ui._selected.serial == "R58M12345")
    check("SEND is disabled for a non-root device", not ui.btn_send._enabled)
    check("a non-root device is not shown as SUPPORTED",
          SupportLevel.SUPPORTED.value not in ui.target_verdict.cget("text"))
    check("the operator is told root is required",
          "ROOT_REQUIRED" in ui.target_verdict.cget("text"))

    ui._on_device_selected("emulator-5554")
    pump(root, 0.05)

    print("send gate is surfaced in the interface")
    check("gate starts clear", ui._gate_clear())
    controller._transactions["emulator-5554"] = TransactionState.DELIVERED
    ui._update_target_panel()
    pump(root, 0.05)
    check("a delivered alert closes the gate", not ui._gate_clear())
    check("SEND is disabled while the gate is closed", not ui.btn_send._enabled)
    check("acknowledge becomes available", ui.btn_ack._enabled)
    check("the operator is told to dismiss on the device",
          "Acknowledge" in ui.target_verdict.cget("text"))
    controller._transactions["emulator-5554"] = TransactionState.READY
    ui._update_target_panel()
    check("clearing the gate re-enables SEND", ui.btn_send._enabled)

    print("log rendering")
    ui.append_log("hello from the test", "ok")
    ui.append_log("a problem", "err")
    text = ui.log_text.get("1.0", "end")
    check("log records a normal line", "hello from the test" in text)
    check("log records an error line", "a problem" in text)

    print("result rendering")
    ok_result = SendResult(
        state=AlertState.ALERT_DISPLAYED,
        device_serial="emulator-5554",
        evidence=["CellBroadcastReceiver.onReceive -- message accepted by the receiver"],
    )
    ui._render_result(ok_result)
    pump(root, 0.05)
    check("a displayed alert is reported as success",
          ui.result_pill._value.cget("text") == "ALERT DISPLAYED")
    check("success is coloured as ok", ui.result_pill._value.cget("fg") == OK)
    check("the success banner names the device",
          "emulator-5554" in ui.outcome_banner._label.cget("text"))
    check("the success banner says to dismiss on the device",
          "DISMISS IT ON THE DEVICE" in ui.outcome_banner._label.cget("text"))
    check("the safety statement survives a success banner",
          "NO CELLULAR TRANSMISSION" in ui.safety_banner._label.cget("text"))

    blocked = SendResult(
        state=AlertState.FAILED,
        failure=FailureCode.DUPLICATE_SEND_BLOCKED,
        device_serial="emulator-5554",
        message="the previous test alert is still outstanding on this device",
    )
    ui._render_result(blocked)
    pump(root, 0.05)
    check("a blocked send is reported as a failure",
          ui.result_pill._value.cget("text") == "DUPLICATE_SEND_BLOCKED")
    check("a blocked send is coloured as an error",
          ui.result_pill._value.cget("fg") == ERR)
    check("a blocked send explains itself to the operator",
          "stack a second dialog" in ui.log_text.get("1.0", "end"))
    check("the safety statement survives a failure banner",
          "NO CELLULAR TRANSMISSION" in ui.safety_banner._label.cget("text"))

    print("stop semantics")
    ui._busy = False
    ui.on_stop()
    body = ui.log_text.get("1.0", "end")
    check("stop on an idle controller says nothing is in progress",
          "Nothing is in progress" in body)

    ui._busy = True
    ui.btn_stop.set_enabled(True)
    ui.on_stop()
    body = ui.log_text.get("1.0", "end")
    check("stop while busy cancels", ui._cancel.is_set())
    check("stop tells the truth about remote dismissal",
          "does not permit remote dismissal" in body)
    check("stop points at the on-device control", "on-device control" in body)
    ui._busy = False

    print("channel is displayed and locked")
    check("category is the ETWS test channel", SERVICE_CATEGORY == 4355)

    print("no hazard-selection surface in the UI")
    surface = all_text(ui.root).lower()
    for forbidden in ("presidential", "extreme threat", "amber alert", "imminent danger"):
        check(f"no {forbidden!r} anywhere in the interface", forbidden not in surface)
    check("the only alert type offered is the test type", "etws test" in surface)

    root.destroy()

    print()
    if FAILURES:
        print(f"{len(FAILURES)} check(s) FAILED")
        return 1
    print("all UI checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())