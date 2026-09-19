#!/usr/bin/env python3
"""Entry point for the Emergency-Simulator desktop application.

    python app/main.py            (from the repository root)
    Emergency-Simulator.exe       (packaged build)

The application triggers a TEST emergency alert through Android's genuine Cell Broadcast pipeline on
a rooted device you control. It performs no cellular transmission of any kind.
"""

from __future__ import annotations

import sys
from pathlib import Path

# Packaged builds run from a temporary directory, so make the repository root importable both ways.
_HERE = Path(__file__).resolve().parent
if str(_HERE.parent) not in sys.path:
    sys.path.insert(0, str(_HERE.parent))
if str(_HERE) not in sys.path:
    sys.path.insert(0, str(_HERE))


def _selftest(argv: list) -> int:
    """Report what a build can actually resolve, without touching a device.

    This is the check that distinguishes a build which merely succeeded from one that works. A frozen
    executable has to find its own injector and, in a self-contained release, its own adb; both live
    inside the PyInstaller archive and are only visible through the runtime layout. Running it here
    lets the build script verify a release rather than assume it.

    The released executable is built as a GUI application, so on Windows it has no console and
    ``sys.stdout`` is None. The report therefore goes to a file when it cannot go to a stream, and the
    path of that file is reported on stderr. Accepting an explicit path covers both cases:

        Emergency-Simulator.exe --selftest [report.txt]
    """


    from app import __version__
    from app.runtime import locate_adb, runtime_layout

    layout = runtime_layout()
    lines = ["Emergency-Simulator self-test"]
    lines.append(f"  version     : {__version__}")
    lines.append(f"  frozen      : {layout.is_frozen}")
    lines.append(f"  bundle root : {layout.bundle_root}")
    for i, root in enumerate(layout.search_roots):
        lines.append(f"  search root {i}: {root}")

    jar = layout.injector_jar
    if jar is None:
        lines.append("  injector    : MISSING")
    else:
        lines.append(f"  injector    : FOUND  {jar}  ({jar.stat().st_size} bytes)")

    bundled = layout.bundled_adb()
    lines.append(f"  bundled adb : {bundled if bundled else 'not bundled'}")

    candidate = locate_adb()
    if candidate.works:
        lines.append(f"  adb         : {candidate.mode.value}  {candidate.path}")
        lines.append(f"  adb version : {candidate.version}")
    else:
        lines.append(f"  adb         : MISSING  {candidate.problem}")

    ok = jar is not None and candidate.works
    lines.append("")
    lines.append("RESULT: " + ("OK" if ok else "INCOMPLETE"))
    report = "\n".join(lines) + "\n"

    explicit = None
    for arg in argv:
        if arg.startswith("--selftest="):
            explicit = arg.split("=", 1)[1]
            # The path may have been quoted by the caller (Windows temp paths can contain spaces), and
            # it arrives here with the quotes still attached because we do our own argv parsing.
            if len(explicit) >= 2 and explicit[0] == explicit[-1] and explicit[0] in ("'", '"'):
                explicit = explicit[1:-1]
    if explicit:
        destination = explicit
    elif sys.stdout is None:
        base = Path(sys.executable).resolve().parent if getattr(sys, "frozen", False) \
            else Path.cwd()
        destination = str(base / "selftest-report.txt")
    else:
        destination = None

    if destination is None:
        sys.stdout.write(report)
        sys.stdout.flush()
    else:
        try:
            Path(destination).write_text(report, encoding="utf-8")
        except OSError as exc:
            if sys.stderr is not None:
                sys.stderr.write(f"could not write the self-test report: {exc}\n")
            return 1
        if sys.stderr is not None:
            sys.stderr.write(f"self-test report written to {destination}\n")

    return 0 if ok else 1


def main() -> int:
    if any(arg == "--selftest" or arg.startswith("--selftest=") for arg in sys.argv[1:]):
        return _selftest(sys.argv[1:])

    try:
        from app.ui import main as ui_main
    except ImportError:
        # Running with the app directory itself on sys.path (packaged layout).
        from ui import main as ui_main  # type: ignore

    return ui_main()


if __name__ == "__main__":
    sys.exit(main())