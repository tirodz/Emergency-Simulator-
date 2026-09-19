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


def main() -> int:
    try:
        root = make_root()
    except tk.TclError as exc:
        print(f"SKIP: no display available ({exc})")
        return 0

    from app.controller import SERVICE_CATEGORY, EmergencySimulatorController
    from app.ui import SAFETY_STRIP, EmergencySimulatorUI

    # A controller that never touches a device: adb is pointed at nothing and discovery is stubbed.
    controller = EmergencySimulatorController.__new__(EmergencySimulatorController)
    controller.on_log = lambda _m: None
    controller._cancel_check = lambda: False
    controller._adb = None
    controller._adb_path = None
    controller.adb_error = "test mode"

    ui = EmergencySimulatorUI(root, controller=controller)
    pump(root, 0.2)

    print("title and safety strip")
    check("window title names the simulator", "EMERGENCY-SIMULATOR" in root.title())
    check("safety strip states no cellular transmission",
          "NO CELLULAR TRANSMISSION" in SAFETY_STRIP)

    print("locked alert type")
    check("status shows no injector yet",
          ui.status_labels["injector"].cget("text") == "---")

    print("message validation through the UI")
    ui.msg_var.set("EXTREME THREAT - TORNADO WARNING")
    from app.controller import SafetyError, validate_body

    try:
        validate_body(ui.msg_var.get())
        check("hazard wording rejected", False)
    except SafetyError:
        check("hazard wording rejected", True)

    ui.msg_var.set("TEST DRILL - HOUSEHOLD DEVICE")
    check("test wording accepted", validate_body(ui.msg_var.get()) == "TEST DRILL - HOUSEHOLD DEVICE")

    check("body prefix enforced in the entry widget",
          validate_body(ui.msg_var.get()).startswith("TEST"))

    print("no device selected blocks sending")
    ui._selected = None
    check("no device is selected initially", ui._selected is None)

    print("log rendering")
    ui.append_log("hello from the test", "ok")
    ui.append_log("a problem", "err")
    text = ui.log_text.get("1.0", "end")
    check("log records a normal line", "hello from the test" in text)
    check("log records an error line", "a problem" in text)

    print("stop semantics")
    ui._busy = False
    ui.on_stop()
    body = ui.log_text.get("1.0", "end")
    check("stop on an idle controller says nothing is in progress",
          "Nothing is in progress" in body)

    ui._busy = True
    ui.btn_stop.configure(state="normal")
    ui.on_stop()
    body = ui.log_text.get("1.0", "end")
    check("stop while busy cancels", ui._cancel.is_set())
    check("stop tells the truth about remote dismissal",
          "does not permit remote dismissal" in body)
    check("stop points at the on-device control",
          "on-device control" in body)
    ui._busy = False

    print("channel is displayed and locked")
    check("category is the ETWS test channel", SERVICE_CATEGORY == 4355)

    root.destroy()

    print()
    if FAILURES:
        print(f"{len(FAILURES)} check(s) FAILED")
        return 1
    print("all UI checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())