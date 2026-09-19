#!/usr/bin/env python3
"""Tests for the safety invariants and the result-detection logic.

These run without a device: they exercise the parts that must never regress -- the body validator,
the fixed channel, and the logcat evidence state machine.

    python3 tools/test_controller.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

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


def main() -> int:
    test_channel_is_fixed()
    test_body_validation()
    test_evidence_patterns()
    test_no_hazard_vocabulary_in_exposed_cli()
    print()
    if FAILURES:
        print(f"{len(FAILURES)} check(s) FAILED")
        return 1
    print("all checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())