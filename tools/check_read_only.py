#!/usr/bin/env python3
"""Guard against state-changing ADB commands being reintroduced into the desktop controller.

Why this exists
---------------

`adb root` is not a query. It tells the device's adbd daemon to restart as root, which is a change
to the phone's state. It was being called during routine device discovery — every Refresh — with no
approval for that specific command, in a project whose stated constraint is that anything that
could change phone state needs the operator's explicit approval.

It was also pointless for the thing it appeared to enable. Whether the AOSP test receiver exists is
decided by `ro.debuggable` at class initialisation, and root does not change a build property. So
the call changed the operator's phone without opening the path it was added for.

That defect was fixed, but nothing stopped it coming back. This check does: it fails if any
state-changing invoker appears in the Rust sources. It is deliberately narrow — it looks for the
specific commands that change phone state, not for the word "root", which appears legitimately in
`ro.debug`, uid comparisons and comments.

Usage
-----

    python3 tools/check_read_only.py            # exit 1 on a violation
    python3 tools/check_read_only.py --verbose

The check is structural, not a proof. It cannot see a command built by string concatenation at
runtime; it catches the direct invocations, which is how this defect appeared both times.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
RUST_SOURCES = [REPO / "src-tauri" / "src"]

#: ADB invocations that change the device or its state. Each entry is (regex, explanation).
#:
#: `shell` is not here: shell commands are how the controller reads the device, and a blanket ban
#: would be useless. The individual state-changing shell verbs are checked separately below.
FORBIDDEN: list[tuple[str, str]] = [
    (
        r'\[\s*"-s"\s*,\s*[a-z_\s]*serial[^]]*"root"\s*\]',
        "adb root restarts adbd as root on the phone; it is a state change, not a query",
    ),
    (
        r'\[\s*"-s"\s*,\s*[a-z_\s]*serial[^]]*"unroot"\s*\]',
        "adb unroot restarts adbd; it is a state change, not a query",
    ),
    (
        r'\[\s*"-s"\s*,\s*[a-z_\s]*serial[^]]*"remount"\s*\]',
        "adb remount changes partition mount state on the phone",
    ),
    (
        r'\[\s*"-s"\s*,\s*[a-z_\s]*serial[^]]*"reboot"[^]]*\]',
        "adb reboot restarts the phone",
    ),
    (
        r'\[\s*"-s"\s*,\s*[a-z_\s]*serial[^]]*"uninstall"\s*\]',
        "adb uninstall removes a package from the phone",
    ),
    (
        r'\[\s*"-s"\s*,\s*[a-z_\s]*serial[^]]*"install-multi-package"[^]]*\]',
        "adb install-multi-package installs onto the phone",
    ),
    (
        r'\[\s*"-s"\s*,\s*[a-z_\s]*serial[^]]*"shell"\s*,\s*"pm"\s*,\s*"clear"',
        "pm clear erases app data on the phone",
    ),
    (
        r'"pm"\s*,\s*"uninstall"',
        "pm uninstall removes a package from the phone",
    ),
    (
        r'"(reboot|flash|format|wipe|factory)"\s*,',
        "flashing, formatting or wiping changes the phone irreversibly",
    ),
    (
        r'"settings"\s*,\s*"put"',
        "settings put writes device settings; the controller must stay read-only outside an "
        "approved, explicitly-named command",
    ),
    (
        r'"svc"\s*,\s*"(power|data|wifi|bluetooth)"',
        "svc toggles radios or power on the phone",
    ),
]

#: Comments and doc examples are not code. A line is skipped when it is a comment, so the guard
#: does not fire on the documentation that explains why these are forbidden.
COMMENT = re.compile(r"^\s*//")


def scan_source(path: Path) -> list[tuple[int, str, str]]:
    violations: list[tuple[int, str, str]] = []
    for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        if COMMENT.match(line):
            continue
        for pattern, explanation in FORBIDDEN:
            if re.search(pattern, line):
                violations.append((number, line.strip(), explanation))
    return violations


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--verbose", action="store_true", help="list every source file scanned")
    args = parser.parse_args()

    files = sorted(path for root in RUST_SOURCES for path in root.rglob("*.rs"))
    if not files:
        print("no Rust sources found; run from the repository root", file=sys.stderr)
        return 2

    if args.verbose:
        for path in files:
            print(f"scanned {path.relative_to(REPO)}")

    total = 0
    for path in files:
        for number, line, explanation in scan_source(path):
            total += 1
            print(f"{path.relative_to(REPO)}:{number}: {explanation}")
            print(f"    {line}")

    if total:
        print(
            f"\n{total} state-changing ADB command(s) found. These change the operator's phone and "
            "need explicit approval for the specific command; the controller's discovery path must "
            "stay read-only. If one of these is genuinely required, it belongs behind an individual "
            "confirmation, not in a refresh.",
            file=sys.stderr,
        )
        return 1

    print(f"OK — no state-changing ADB commands in {len(files)} Rust source files")
    return 0


if __name__ == "__main__":
    sys.exit(main())
