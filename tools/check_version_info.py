#!/usr/bin/env python3
"""Keep the Windows version resource and the application version in step.

The PE version resource is what a person or a scanner reads off the binary without running it. It is
also invisible to the test suite, so it drifts silently. This check makes the drift a failure.

    python3 tools/check_version_info.py
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
INIT = REPO / "app" / "__init__.py"
VERSION_INFO = REPO / "packaging" / "version_info.txt"


def main() -> int:
    version = re.search(r'__version__\s*=\s*"([^"]+)"', INIT.read_text())
    if not version:
        print(f"FAIL: no __version__ in {INIT}", file=sys.stderr)
        return 1
    text = VERSION_INFO.read_text()

    parts = [int(p) for p in version.group(1).split(".")]
    while len(parts) < 4:
        parts.append(0)
    expected = ".".join(str(p) for p in parts)

    problems = []
    if f"filevers={tuple(parts)}" not in text:
        problems.append(f"filevers should be {tuple(parts)}")
    if f"prodvers={tuple(parts)}" not in text:
        problems.append(f"prodvers should be {tuple(parts)}")
    if f"StringStruct('FileVersion', '{expected}')" not in text:
        problems.append(f"FileVersion should be '{expected}'")
    if f"StringStruct('ProductVersion', '{expected}')" not in text:
        problems.append(f"ProductVersion should be '{expected}'")

    if problems:
        print(f"FAIL: packaging/version_info.txt does not match app/__init__.py "
              f"(__version__ = {version.group(1)})", file=sys.stderr)
        for p in problems:
            print(f"  {p}", file=sys.stderr)
        return 1

    print(f"ok: version resource matches __version__ {version.group(1)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())