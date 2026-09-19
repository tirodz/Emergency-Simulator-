#!/usr/bin/env python3
"""Command-line front end for the Emergency-Simulator test-alert controller.

The Android alert channel is locked to the ETWS test category. There is no option, here or anywhere
below, that can select a real hazard class, and no path that can transmit anything.

Examples:
    python tools/test_alert.py --list
    python tools/test_alert.py --dry-run
    python tools/test_alert.py --device emulator-5554
    python tools/test_alert.py --message "TEST DRILL - HOUSEHOLD DEVICE" --yes
"""

from __future__ import annotations

import argparse
import logging
import sys
from pathlib import Path
from typing import List, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from app import BANNER, __version__  # noqa: E402
from app.controller import (  # noqa: E402
    DEFAULT_BODY,
    SERVICE_CATEGORY,
    EmergencySimulatorController,
    SafetyError,
)
from app.logsetup import configure_logging  # noqa: E402
from app.models import AlertState, DeviceState  # noqa: E402

EXIT_OK = 0
EXIT_FAILURE = 1
EXIT_USAGE = 2
EXIT_REFUSED = 3
EXIT_CANCELLED = 4


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        prog="test_alert.py",
        description=(
            "Trigger a TEST emergency alert through Android's genuine Cell Broadcast pipeline on a "
            "rooted development device. No cellular transmission occurs."
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=(
            "The alert channel is fixed to the ETWS test channel "
            f"{SERVICE_CATEGORY} (0x1103) and cannot be changed.\n"
            "The message must begin with 'TEST'.\n\n"
            "ADB: set ADB_PATH or pass --adb. Obtain Platform Tools from\n"
            "  https://developer.android.com/tools/releases/platform-tools"
        ),
    )
    p.add_argument("--list", action="store_true", help="list attached devices and exit")
    p.add_argument("--device", metavar="SERIAL", help="target device serial")
    p.add_argument("--adb", metavar="PATH", help="path to the adb executable")
    p.add_argument(
        "--message",
        metavar="TEXT",
        default=DEFAULT_BODY,
        help=f"test alert body; must begin with TEST (default: {DEFAULT_BODY!r})",
    )
    p.add_argument(
        "--dry-run",
        action="store_true",
        help="run every check but change nothing on the device and send nothing",
    )
    p.add_argument(
        "--yes",
        action="store_true",
        help="skip the interactive confirmation (for scripted, already-confirmed use)",
    )
    p.add_argument("--log-dir", metavar="DIR", help="directory for the persistent log file")
    p.add_argument("--verbose", action="store_true", help="also log to stderr")
    p.add_argument("--version", action="version", version=f"Emergency-Simulator {__version__}")
    return p


def print_devices(devices) -> None:
    if not devices:
        print("No devices attached.")
        print("  Start an emulator, or connect a device with USB debugging enabled.")
        return
    print(f"{'SERIAL':<22}{'STATE':<15}{'ANDROID':<10}{'ROOT':<7}CELLBROADCAST")
    for d in devices:
        root = "yes" if d.is_root else "no"
        cb = d.cellbroadcast_package or "not found"
        android = d.release or "?"
        print(f"{d.serial:<22}{d.state.value:<15}{android:<10}{root:<7}{cb}")
        for note in d.notes:
            print(f"    note: {note}")
    ready = [d for d in devices if d.is_usable]
    print()
    if ready:
        print(f"{len(ready)} device(s) READY for a test alert.")
    else:
        print("No device is READY. Review the STATE column above.")


def confirm(device_label: str, release: str, sdk: str, body: str, verbose: bool) -> bool:
    print()
    print("WARNING:")
    print("This will trigger a TEST emergency alert on the selected rooted device.")
    print()
    print(f"  Target:   {device_label}")
    print(f"  Android:  {release} / API {sdk}")
    print(f"  Type:     ETWS TEST")
    print(f"  Channel:  {SERVICE_CATEGORY} (locked)")
    print(f"  Message:  {body}")
    print()
    print("This is a test on a device you control. No cellular transmission occurs.")
    print()
    try:
        answer = input("Send test alert? [y/N]: ")
    except (EOFError, KeyboardInterrupt):
        print()
        return False
    return answer.strip().lower() in ("y", "yes")


def main(argv: Optional[List[str]] = None) -> int:
    args = build_parser().parse_args(argv)
    log_file = configure_logging(
        Path(args.log_dir) if args.log_dir else None,
        to_console=args.verbose,
    )

    if not args.list:
        print(BANNER)
        print(f"log: {log_file}")
        print()

    try:
        controller = EmergencySimulatorController(
            adb_path=args.adb,
            on_log=(lambda m: print(f"  {m}")) if not args.list else (lambda _m: None),
        )
    except Exception as exc:  # adb missing is reported clearly, not as a traceback
        print(f"ERROR: {exc}", file=sys.stderr)
        return EXIT_FAILURE

    if args.list:
        try:
            print_devices(controller.discover())
        except Exception as exc:
            print(f"ERROR: {exc}", file=sys.stderr)
            return EXIT_FAILURE
        return EXIT_OK

    # Validate the body before touching any device.
    from app.controller import validate_body

    try:
        body = validate_body(args.message)
    except SafetyError as exc:
        print(f"REFUSED: {exc}", file=sys.stderr)
        return EXIT_REFUSED

    # Resolve and inspect the device.
    try:
        device = controller.select_device(args.device)
    except SafetyError as exc:
        if str(exc) == "NO_DEVICE":
            print("NO_DEVICE: no Android device is attached.", file=sys.stderr)
            print("  Start an emulator, or connect a device with USB debugging enabled.",
                  file=sys.stderr)
            return EXIT_FAILURE
        print(f"ERROR: {exc}", file=sys.stderr)
        return EXIT_FAILURE

    print(controller.describe_device(device))
    print()
    print(f"Channel: {SERVICE_CATEGORY} (0x1103, ETWS test -- locked)")
    print(f"Message: {body}")
    print()

    if not device.is_usable:
        reason = {
            DeviceState.OFFLINE: "DEVICE_OFFLINE",
            DeviceState.UNAUTHORIZED: "DEVICE_UNAUTHORIZED",
            DeviceState.BUSY: "DEVICE_BUSY",
            DeviceState.NO_ROOT: "NO_ROOT",
            DeviceState.UNSUPPORTED: "DEVICE_UNSUPPORTED",
        }.get(device.state, "NOT_READY")
        print(f"{reason}: this device cannot be used for a test alert.", file=sys.stderr)
        for note in device.notes:
            print(f"  {note}", file=sys.stderr)
        if device.state is DeviceState.NO_ROOT:
            print("  Root is required: the emergency broadcast is a protected broadcast,", file=sys.stderr)
            print("  and the framework rejects any non-root sender.", file=sys.stderr)
        return EXIT_FAILURE

    if args.dry_run:
        print("[dry-run] No device configuration will be changed and nothing will be sent.")
        print()

    if not args.dry_run and not args.yes:
        if not confirm(device.label, device.release, device.sdk, body, args.verbose):
            print("Cancelled. Nothing was sent.")
            return EXIT_CANCELLED

    result = controller.send_test_alert(device, body=body, dry_run=args.dry_run)

    print()
    if result.ok:
        print("SUCCESS -- the genuine Android emergency alert was displayed.")
        for line in result.evidence:
            print(f"  - {line}")
        print()
        print("Dismiss it using the alert's own on-device control.")
        print("Remote dismissal is not supported by Android.")
        return EXIT_OK

    if result.state is AlertState.CANCELLED:
        print("CANCELLED -- nothing was delivered.")
        return EXIT_CANCELLED

    if result.state is AlertState.READY_TO_SEND:
        print("DRY RUN COMPLETE -- every check passed. Nothing was sent.")
        return EXIT_OK

    print(f"FAILED -- {result.failure.value if result.failure else 'UNKNOWN'}")
    print(f"  {result.message}")
    if result.evidence:
        for line in result.evidence:
            print(f"  - {line}")
    if result.injector_exit_code is not None:
        print(f"  injector exit code: {result.injector_exit_code}")
    if result.injector_stderr.strip():
        print("  injector stderr:")
        for line in result.injector_stderr.strip().splitlines()[-5:]:
            print(f"    {line}")
    print()
    print("A clean injector exit code does not by itself mean the alert was shown;")
    print("the result above is based on the Cell Broadcast log evidence.")
    return EXIT_FAILURE


if __name__ == "__main__":
    sys.exit(main())