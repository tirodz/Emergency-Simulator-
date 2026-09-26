#!/usr/bin/env python3
"""Structural checks on the PowerShell the operator is asked to paste.

Why this exists
---------------

The one-paste block is the operator's only way in, and this environment has no PowerShell, so the
script has never actually been executed. That gap produced a real failure: ``Join-Path
$PSScriptRoot`` threw on the first run, because ``$PSScriptRoot`` is empty when a block is pasted
into the console rather than run from a ``.ps1`` file.

These checks cannot prove the script runs. They catch the specific, mechanical defects that a read
of the diff misses: characters that terminate a block mid-stream, a ``return`` outside the wrapper
that would kill the console, and empty-base ``Join-Path`` calls.

Run before handing the paste to the operator:

    python3 tools/check_paste_ps1.py
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
SOURCES = [
    REPO / "docs" / "connect-one-paste.md",
    REPO / "docs" / "connect.ps1",
]

#: Inside a single-quoted PowerShell string a backtick is literal, so ``'n'`` is a legal two-char
#: string and must not be flagged as a stray escape.
SINGLE_QUOTED = re.compile(r"'[^']*'")

#: PowerShell's real escapes. Anything else after a backtick prompts for input inside a double-quoted
#: string, which is what would swallow the rest of a pasted block.
VALID_ESCAPES = set("0abefnrtv$`\"\\")

#: ``$env:`` variables are conventionally guarded by an ``if ($env:X)`` test, and ``$PSScriptRoot``
#: is the one that was actually observed to be empty in a pasted block.
ENV_VAR = re.compile(r"^\$env:")
SCRIPT_ROOT = re.compile(r"^\$PSScriptRoot\b")

FAILURES: list[str] = []


def check(powershell: str, label: str) -> None:
    # -- Characters that would terminate a pasted block -------------------------------
    for match in re.finditer(r'"[^"\n]*`(.?)[^"\n]*"', powershell):
        escape = match.group(1)
        if escape and escape not in VALID_ESCAPES:
            FAILURES.append(
                f"{label}: backtick-{escape!r} is not a PowerShell escape; inside a double-quoted "
                "string it can prompt for input and swallow the rest of the paste"
            )

    # A literal tab inserted where a PowerShell escape was meant.
    if re.search(r'"(?:[^"\n]*)\t(?:[^"\n]*)"', powershell):
        FAILURES.append(f"{label}: literal TAB inside a double-quoted string")

    # -- Statement-level structure -----------------------------------------------------
    # ``return`` is only legal inside a function or scriptblock. In the pasted block that is the
    # top-level ``& { ... }`` wrapper, so an odd nesting depth means a return would abort the
    # console instead of just the block.
    depth = 0
    for number, line in enumerate(powershell.splitlines(), start=1):
        stripped = line.strip()
        if stripped.startswith("#"):
            continue
        if re.match(r"^return\b", stripped) and depth == 0:
            FAILURES.append(f"{label}:{number}: 'return' at top level would close the console")
        depth += line.count("{") - line.count("}")

    if depth != 0:
        FAILURES.append(f"{label}: brace imbalance of {depth} — a block was left open or closed early")

    # -- Join-Path with a base that can be empty ---------------------------------------
    # Only the cases that are genuinely unguarded: the script root, and a plain variable that is
    # never assigned anywhere in the script.
    assigned = set(re.findall(r"^\s*\$(\w+)\s*=", powershell, re.MULTILINE))
    assigned |= set(re.findall(r"foreach\s*\(\s*\$(\w+)\s+in\b", powershell))
    assigned |= set(re.findall(r"\$(\w+)\s+in\s+@\(", powershell))
    for number, line in enumerate(powershell.splitlines(), start=1):
        if "Join-Path" not in line:
            continue
        after = line.split("Join-Path", 1)[1].strip()
        if not after:
            continue
        first_arg = after.split()[0]
        if SCRIPT_ROOT.match(first_arg):
            FAILURES.append(
                f"{label}:{number}: Join-Path {first_arg} — $PSScriptRoot is empty when a block is "
                "pasted into the console, and Join-Path rejects an empty base"
            )
        elif ENV_VAR.match(first_arg):
            continue  # conventionally guarded by an `if ($env:X)` test
        elif first_arg.startswith("$(") or first_arg.startswith(("'", '"')):
            continue
        elif first_arg.startswith("$"):
            name = first_arg.lstrip("$").rstrip(")")
            if name not in assigned:
                FAILURES.append(
                    f"{label}:{number}: Join-Path {first_arg} — never assigned in this script, so "
                    "PowerShell rejects the call when it is empty"
                )

    # -- Array indexing of a possibly-scalar pipeline result ---------------------------
    for number, line in enumerate(powershell.splitlines(), start=1):
        if re.search(r"^\s*\$\w+\s*=\s*\(?&\s", line) and "|" in line and "@(" not in line:
            FAILURES.append(
                f"{label}:{number}: pipeline result assigned without @() — a single match becomes a "
                "scalar and [0] would index a character"
            )

    # -- Reads of a script-scoped variable that is never written ------------------------
    # ``$script:X`` is how this block carries state between its steps, so a read of one that is
    # never assigned is always a defect: the step silently takes its `else` branch. This was a real
    # one — step 5 tested ``$script:Device`` to decide whether to ask the operator to confirm the
    # phone was unchanged, the variable was never set, so the check never ran and the operator was
    # always told "no phone attached". Reading is matched as a bare ``$script:X`` that is not
    # immediately followed by ``=``.
    script_writes = set(re.findall(r"\$script:(\w+)\s*=[^=]", powershell))
    for number, line in enumerate(powershell.splitlines(), start=1):
        if line.strip().startswith("#"):
            continue
        # A plain \w+ with a trailing lookahead backtracks and truncates the name (AdbPath -> AdbPat),
        # so take the full word first and inspect what follows it.
        for match in re.finditer(r"\$script:(\w+)", line):
            name = match.group(1)
            if re.match(r"\s*=[^=]", line[match.end():]):
                continue  # this occurrence is the assignment itself
            if name not in script_writes:
                FAILURES.append(
                    f"{label}:{number}: $script:{name} is read but never assigned — the branch that "
                    "depends on it never runs"
                )


def extract_block(text: str) -> str | None:
    """Return the ```powershell fenced block, or None if the document has no such block."""
    match = re.search(r"```powershell\n(.*?)\n```", text, re.DOTALL)
    return match.group(1) if match else None


def main() -> int:
    for path in SOURCES:
        if not path.is_file():
            FAILURES.append(f"{path}: missing")
            continue
        text = path.read_text(encoding="utf-8")
        if path.suffix == ".md":
            block = extract_block(text)
            if block is None:
                FAILURES.append(f"{path}: no ```powershell block found")
                continue
            check(block, path.name)
        else:
            check(text, path.name)

    if FAILURES:
        print("FAIL")
        for failure in FAILURES:
            print("  -", failure)
        return 1

    print("OK — no mechanical defects found in the pasted PowerShell")
    print("    (structure only; this does not execute the script)")
    return 0


if __name__ == "__main__":
    sys.exit(main())