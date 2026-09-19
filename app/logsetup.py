"""Logging: a persistent local log file plus an optional console stream.

The log records what the operator needs during a test session and nothing more. It deliberately
never records device file contents beyond the two boolean flags we act on.
"""

from __future__ import annotations

import logging
import sys
from pathlib import Path
from typing import Optional

from .controller import default_log_dir

_FORMAT = "%(asctime)s  %(levelname)-7s  %(message)s"
_DATEFMT = "%Y-%m-%d %H:%M:%S"


def configure_logging(
    log_dir: Optional[Path] = None,
    to_console: bool = False,
    level: int = logging.INFO,
) -> Path:
    """Attach a file handler (and optionally a console handler). Returns the log file path."""
    directory = Path(log_dir) if log_dir else default_log_dir()
    directory.mkdir(parents=True, exist_ok=True)
    log_file = directory / "emergency-simulator.log"

    root = logging.getLogger("emergency_simulator")
    root.setLevel(level)
    root.handlers.clear()
    root.propagate = False

    file_handler = logging.FileHandler(log_file, encoding="utf-8")
    file_handler.setFormatter(logging.Formatter(_FORMAT, _DATEFMT))
    root.addHandler(file_handler)

    if to_console:
        console = logging.StreamHandler(sys.stderr)
        console.setFormatter(logging.Formatter(_FORMAT, _DATEFMT))
        root.addHandler(console)

    return log_file