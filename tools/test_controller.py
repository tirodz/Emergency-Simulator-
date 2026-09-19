#!/usr/bin/env python3
"""Tests for the safety invariants and the result-detection logic.

These run without a device: they exercise the parts that must never regress -- the body validator,
the fixed channel, and the logcat evidence state machine.

    python3 tools/test_controller.py
"""

from __future__ import annotations

import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

sys.path.insert(0, str(REPO))

from app.controller import (  # noqa: E402
    DEFAULT_BODY,
    EVIDENCE_MARKERS,
    FILTER_MARKERS,
    SERVICE_CATEGORY,
    SafetyError,
    validate_body,
)

FAILURES = []


def check(name: str, condition: bool, detail: str = "") -> None:
    if condition:
        print(f"  ok    {name}")
    else:
        print(f"  FAIL  {name} {detail}")
        FAILURES.append(name)


def test_channel_is_fixed() -> None:
    print("channel")
    check("service category is the ETWS test channel", SERVICE_CATEGORY == 4355)
    # A real hazard class must not be reachable. There is no parameter for it, and this asserts the
    # constant itself is the test channel and not something else.
    check("4355 is 0x1103", SERVICE_CATEGORY == 0x1103)


def test_body_validation() -> None:
    print("body validation")
    check("default body passes", validate_body(DEFAULT_BODY) == DEFAULT_BODY)
    check("empty becomes default", validate_body("") == DEFAULT_BODY)
    check("whitespace becomes default", validate_body("   ") == DEFAULT_BODY)
    check("collapses internal whitespace",
          validate_body("TEST  ALERT   -   SIM") == "TEST ALERT - SIM")
    check("TEST prefix accepted", validate_body("TEST DRILL - HOUSEHOLD") == "TEST DRILL - HOUSEHOLD")

    for bad in (
        "EXTREME THREAT - TORNADO",
        "PRESIDENTIAL ALERT",
        "AMBER ALERT - CHILD ABDUCTION",
        "CBRNE HAZARD RELEASE",
        "test alert",          # lowercase is not accepted: the on-screen text must be unambiguous
        "ALERT - TEST",
    ):
        try:
            validate_body(bad)
            check(f"rejects {bad!r}", False, "-- was accepted")
        except SafetyError:
            check(f"rejects {bad!r}", True)

    try:
        validate_body("TEST " + "x" * 400)
        check("rejects over-long body", False)
    except SafetyError:
        check("rejects over-long body", True)


def test_evidence_patterns() -> None:
    print("evidence detection")
    real_logcat = """
09-19 13:10:00.000  1000  1000 D CellBroadcastReceiver: onReceive Intent { act=android.provider.action.SMS_EMERGENCY_CB_RECEIVED }
09-19 13:10:00.500  1000  1000 D CBAlertService: onStartCommand: android.provider.action.SMS_EMERGENCY_CB_RECEIVED
09-19 13:10:01.000  1000  1000 D CellBroadcastAlertAudio: Set state from 0 to 1
09-19 13:10:01.500  1000  1000 D CellBroadcastAlertDialog: onCreate loaded message list of size 1
"""
    for name, pattern in EVIDENCE_MARKERS:
        check(f"matches {name} in a real alert log", bool(pattern.search(real_logcat)))

    filtered_logcat = "D CBAlertService: ignoring alert of type 4355 by user preference"
    check("detects preference filtering",
          any(p.search(filtered_logcat) for _n, p in FILTER_MARKERS))

    denial = ("W ActivityManager: Permission Denial: not allowed to send broadcast "
              "android.provider.action.SMS_EMERGENCY_CB_RECEIVED from pid=1, uid=2000")
    check("detects a rejected broadcast",
          any(p.search(denial) for _n, p in FILTER_MARKERS))

    no_extras = "D CBAlertService: received SMS_CB_RECEIVED_ACTION with no extras!"
    check("detects a payload-less broadcast",
          any(p.search(no_extras) for _n, p in FILTER_MARKERS))

    # A clean injector run with no downstream activity must NOT look like success.
    nothing = "I AlertInjector: broadcast sent"
    dialog = dict(EVIDENCE_MARKERS)["CellBroadcastAlertDialog"]
    check("does not claim success on injector output alone", not dialog.search(nothing))


def test_no_hazard_vocabulary_in_exposed_cli() -> None:
    print("cli surface")
    import subprocess

    out = subprocess.run(
        [sys.executable, str(Path(__file__).resolve().parent / "test_alert.py"), "--help"],
        capture_output=True, text=True, timeout=60,
    ).stdout.lower()
    # Options that would let a user select a real alert class must not exist.
    for forbidden in ("--category", "--channel", "--presidential", "--amber", "--hazard",
                      "--emergency-type"):
        check(f"no {forbidden} option", forbidden not in out)


def test_send_gate() -> None:
    """The gate must refuse a send whenever an alert may still be outstanding."""
    print("send gate")
    from app.controller import EmergencySimulatorController
    from app.models import Device, DeviceState, TransactionState

    c = EmergencySimulatorController.__new__(EmergencySimulatorController)
    c.on_log = lambda _m: None
    c._cancel_check = lambda: False
    c._transactions = {}

    serial = "TEST-SERIAL"
    check("a fresh device is READY", c.transaction_state(serial) is TransactionState.READY)
    check("READY is not gated", c._gate(serial) is None)

    for state in (TransactionState.BUSY, TransactionState.DELIVERED, TransactionState.UNCERTAIN):
        c._transactions[serial] = state
        reason = c._gate(serial)
        check(f"{state.value} is gated", reason is not None)
        check(f"{state.value} explains itself", bool(reason and len(reason) > 20))

    c.acknowledge(serial)
    check("acknowledge clears the gate", c._gate(serial) is None)

    # A gated device must be refused by the public entry point too, not just by the helper.
    c._transactions[serial] = TransactionState.DELIVERED
    dev = Device(serial=serial, state=DeviceState.READY, is_root=True,
                 cellbroadcast_package="com.google.android.cellbroadcastreceiver")
    result = c.send_test_alert(dev, body=DEFAULT_BODY, dry_run=True)
    check("send refuses a gated device", not result.ok)
    check("refusal code is DUPLICATE_SEND_BLOCKED",
          result.failure is not None and result.failure.value == "DUPLICATE_SEND_BLOCKED")


def test_adb_probe_rejects_a_non_working_executable() -> None:
    """A file that exists but cannot run must not be reported as a usable adb.

    The property under test is that a present-but-unusable executable is rejected and the reason is
    reported. The *reason* is platform-specific -- a POSIX host gets the program's own exit status,
    Windows gets an OS-level load failure -- so the assertions here must not depend on either.
    """
    print("adb probe")
    import os
    import tempfile

    from app.runtime import probe_adb

    with tempfile.TemporaryDirectory() as tmp:
        not_a_program = Path(tmp) / "fake-adb"
        not_a_program.write_text("this is not an executable\n")
        cand = probe_adb(str(not_a_program))
        check("a file that is not a program is not usable", not cand.works)
        check("the failure is reported", bool(cand.problem))

        # Where the host can execute a script, use one that fails on its own terms and assert the
        # exit status is captured. Windows cannot run a shebang script, so this part is POSIX-only.
        if os.name != "nt":
            script = Path(tmp) / "failing-adb"
            script.write_text("#!/bin/sh\necho 'error while loading shared libraries' >&2\nexit 127\n")
            os.chmod(script, 0o755)
            failing = probe_adb(str(script))
            check("a failing executable is not usable", not failing.works)
            check("the exit status is captured",
                  bool(failing.problem and "127" in failing.problem))
            check("the program's own output is captured",
                  bool(failing.problem and "shared libraries" in failing.problem))

        missing = probe_adb(str(Path(tmp) / "does-not-exist"))
        check("a missing file is not usable", not missing.works)
        check("a missing file says so", bool(missing.problem and "not found" in missing.problem))


def test_runtime_layout_finds_the_injector() -> None:
    """A checkout must be able to locate its own injector through the shared layout."""
    print("runtime layout")
    from app.runtime import runtime_layout

    layout = runtime_layout()
    check("a checkout is not treated as packaged", not layout.is_frozen)
    check("search roots are non-empty", len(layout.search_roots) > 0)
    jar = layout.injector_jar
    check("the injector jar is located", jar is not None and jar.is_file())
    if jar:
        check("the jar is named as expected", jar.name == "alertinject.jar")
        check("the jar is non-trivial in size", jar.stat().st_size > 500)


def test_selftest_argument_parsing() -> None:
    """`--selftest=<path>` must survive a path containing spaces, quoted or not.

    Windows temp paths contain spaces often enough that this is not hypothetical, and the test
    verification runs from `%TEMP%`. The quotes are stripped because the argument is parsed here
    rather than by a shell.
    """
    print("selftest argument")
    import subprocess

    from app.main import _selftest

    with tempfile.TemporaryDirectory() as tmp:
        spaced = Path(tmp) / "dir with spaces"
        spaced.mkdir()
        plain = spaced / "plain.txt"
        quoted = spaced / "quoted.txt"

        rc = _selftest([f"--selftest={plain}"])
        check("a path with spaces is accepted unquoted", plain.is_file())
        check("the report is written there", "RESULT:" in plain.read_text())

        rc2 = _selftest([f'--selftest="{quoted}"'])
        check("a path with spaces is accepted when quoted", quoted.is_file())
        check("the quotes are stripped", "RESULT:" in quoted.read_text())
        check("a literal quoted name is not created", not (spaced / '"quoted.txt"').exists())

        # A real run of the executable, so the argument parsing is exercised as it is in production.
        # Whether adb is available is not asserted: this test is about argument handling, and CI runs
        # it on a host with no adb staged yet.
        real = spaced / "real.txt"
        proc = subprocess.run(
            [sys.executable, str(REPO / "app" / "main.py"), f"--selftest={real}"],
            capture_output=True, text=True, cwd=str(REPO),
        )
        check("the process reports rather than crashing", "Traceback" not in proc.stderr)
        check("the process wrote its report", real.is_file())
        check("the report reached a verdict", "RESULT:" in real.read_text())
        check("the process exits with a verdict, not an error",
              proc.returncode in (0, 1))


def test_capability_classification() -> None:
    """Support level must come from observed facts, not from the device's brand."""
    print("capability classification")
    from app.models import Device, DeviceState, SupportLevel

    unreachable = Device(serial="x", adb_connected=False)
    check("an unreachable device is UNTESTED",
          unreachable.support_level is SupportLevel.UNTESTED)

    no_cb = Device(serial="x", adb_connected=True, state=DeviceState.UNSUPPORTED)
    check("a device without Cell Broadcast is UNSUPPORTED",
          no_cb.support_level is SupportLevel.UNSUPPORTED)

    non_root = Device(serial="x", adb_connected=True, state=DeviceState.NO_ROOT,
                      cellbroadcast_package="com.google.android.cellbroadcastreceiver")
    check("a non-root device needs root",
          non_root.support_level is SupportLevel.ROOT_REQUIRED)

    ready = Device(serial="x", adb_connected=True, state=DeviceState.READY, is_root=True,
                   cellbroadcast_package="com.google.android.cellbroadcastreceiver")
    check("a ready rooted device is SUPPORTED",
          ready.support_level is SupportLevel.SUPPORTED)

    # The same capabilities under a different brand must classify identically.
    for brand in ("samsung", "google", "Xiaomi", "motorola", "Nothing"):
        branded = Device(serial="x", adb_connected=True, state=DeviceState.READY, is_root=True,
                         cellbroadcast_package="com.google.android.cellbroadcastreceiver",
                         manufacturer=brand)
        check(f"{brand} with equal capability classifies the same",
              branded.support_level is SupportLevel.SUPPORTED)


def main() -> int:
    test_channel_is_fixed()
    test_body_validation()
    test_evidence_patterns()
    test_no_hazard_vocabulary_in_exposed_cli()
    test_send_gate()
    test_adb_probe_rejects_a_non_working_executable()
    test_runtime_layout_finds_the_injector()
    test_selftest_argument_parsing()
    test_capability_classification()
    print()
    if FAILURES:
        print(f"{len(FAILURES)} check(s) FAILED")
        return 1
    print("all checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())