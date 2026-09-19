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


def main() -> int:
    try:
        from app.ui import main as ui_main
    except ImportError:
        # Running with the app directory itself on sys.path (packaged layout).
        from ui import main as ui_main  # type: ignore

    return ui_main()


if __name__ == "__main__":
    sys.exit(main())