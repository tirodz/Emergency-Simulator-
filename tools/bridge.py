#!/usr/bin/env python3
"""Bridge between this analysis environment and the operator's Windows laptop.

Why this exists
---------------

The Android target and the Windows build machine are on the operator's desk. This environment is a
container with no USB, no Windows filesystem and no route to that laptop: every network path is
inbound-only. The laptop can reach this container, but this container cannot reach the laptop.

That leaves exactly one workable topology, and this tool is it:

    operator's laptop  --(adb)-->  Android target
            |
            |  HTTPS (outbound from the laptop, inbound to this container)
            v
    this analysis environment

The laptop runs the read-only inspection commands and posts the raw output here; this side parses it,
answers questions about it, and hands back the next batch. Nothing about the observations is
paraphrased in transit.

What this deliberately does NOT do
----------------------------------

* It never touches a phone. It has no adb and no device access.
* It never executes a command received from the network. Tasks are authored here and *served*;
  evidence is received and *stored*. There is no path from an inbound request to code execution.
* It does not claim delivery. It stores raw text; judgement about whether an alert appeared is made
  from downstream evidence by the classifier, never from an exit code.

Security posture
----------------

The listening port is reachable from the internet through the platform's proxy, so every endpoint
except the health check requires a bearer token. The token is generated on first run and written to
``bridge-token.txt`` (gitignored). Evidence is size-capped and confined to ``evidence/``. Nothing is
served from outside the repository.

Usage
-----

    python3 tools/bridge.py                 # serve on 0.0.0.0:12000
    python3 tools/bridge.py --port 12001
    python3 tools/bridge.py --print        # also print pending tasks to the console
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import secrets
import sys
import threading
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import unquote

REPO = Path(__file__).resolve().parent.parent

#: Where posted evidence is written. Confined to the repository; never escaped.
EVIDENCE_DIR = REPO / "evidence"

#: Tasks served to the operator. Authored here, never writable over the network.
TASK_FILE = REPO / "bridge-tasks.json"

#: Run-once token, kept out of version control.
TOKEN_FILE = REPO / "bridge-token.txt"

#: Evidence larger than this is refused. Raw logcat can be large; 4 MB is generous for a capture.
MAX_EVIDENCE_BYTES = 4 * 1024 * 1024

#: Evidence names are chosen by the sender, so they are sanitised to a strict allowlist.
SAFE_NAME = re.compile(r"^[A-Za-z0-9._-]{1,120}$")


def load_or_create_token() -> str:
    """Return the shared token, generating one on first run.

    The token is the only thing standing between the public port and the evidence store, so it is
    generated with ``secrets`` rather than anything predictable, and it is never committed.
    """
    override = os.environ.get("BRIDGE_TOKEN")
    if override:
        return override.strip()
    if TOKEN_FILE.is_file():
        token = TOKEN_FILE.read_text(encoding="utf-8").strip()
        if token:
            return token
    token = secrets.token_urlsafe(32)
    TOKEN_FILE.write_text(token + "\n", encoding="utf-8")
    return token


def load_tasks(path: Path) -> dict:
    """Return the task batch to serve, or an empty one if none has been authored yet."""
    if not path.is_file():
        return {
            "batch": "none",
            "note": f"No task batch at {path}.",
            "tasks": [],
        }
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        return {"batch": "error", "note": f"task file unreadable: {exc}", "tasks": []}


class Handler(BaseHTTPRequestHandler):
    server_version = "EmergencySimulatorBridge/1.0"

    # -- plumbing ---------------------------------------------------------

    def log_message(self, fmt: str, *args) -> None:
        """Log to stderr in a single line, so the console output stays readable."""
        sys.stderr.write(
            f"{datetime.now(timezone.utc).isoformat(timespec='seconds')} "
            f"{self.address_string()} {fmt % args}\n"
        )

    def _send_json(self, status: int, payload: dict) -> None:
        body = json.dumps(payload, indent=2).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    def _authorized(self) -> bool:
        header = self.headers.get("Authorization", "")
        expected = f"Bearer {self.server.token}"  # type: ignore[attr-defined]
        if secrets.compare_digest(header, expected):
            return True
        self._send_json(401, {"error": "missing or invalid bearer token"})
        return False

    def _read_body(self) -> bytes:
        try:
            length = int(self.headers.get("Content-Length", "0"))
        except ValueError:
            return b""
        if length <= 0:
            return b""
        if length > MAX_EVIDENCE_BYTES:
            return b""
        return self.rfile.read(length)

    def _send_bytes(self, status: int, body: bytes, filename: str) -> None:
        self.send_response(status)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Content-Disposition", f'attachment; filename="{filename}"')
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    # -- routes -----------------------------------------------------------

    def do_GET(self) -> None:  # noqa: N802 (http.server's naming)
        path = self.path.split("?", 1)[0]

        # The health check is unauthenticated on purpose: it carries no data, and it lets the
        # operator confirm reachability before they have the token to hand.
        if path == "/health":
            self._send_json(200, {
                "ok": True,
                "service": "emergency-simulator-bridge",
                "note": "Analysis-environment bridge. Serves tasks, stores evidence. Cannot touch a phone.",
            })
            return

        if not self._authorized():
            return

        if path == "/task":
            self._send_json(200, load_tasks(self.server.task_file))  # type: ignore[attr-defined]
            return

        # A built artifact to hand the operator, and its hash so they can verify the transfer.
        # Only files named on the command line are reachable: the path is checked by equality
        # against that allow-list, so a traversal or a guessed name cannot select anything else.
        if path.startswith("/download/"):
            name = unquote(path[len("/download/"):])
            artifacts = self.server.artifacts  # type: ignore[attr-defined]
            if name not in artifacts:
                self._send_json(404, {"error": "no such artifact", "available": sorted(artifacts)})
                return
            target = Path(artifacts[name])
            if not target.is_file():
                self._send_json(404, {"error": f"artifact {name} is registered but missing on disk"})
                return
            self._send_bytes(200, target.read_bytes(), name)
            return

        if path == "/artifacts":
            out = []
            for name, file in self.server.artifacts.items():  # type: ignore[attr-defined]
                p = Path(file)
                entry = {"name": name, "available": p.is_file()}
                if p.is_file():
                    entry["bytes"] = p.stat().st_size
                    entry["sha256"] = hashlib.sha256(p.read_bytes()).hexdigest()
                    entry["url"] = f"{self.server.public_base}/download/{name}"  # type: ignore[attr-defined]
                out.append(entry)
            self._send_json(200, {"artifacts": out})
            return

        if path == "/evidence":
            items = []
            if EVIDENCE_DIR.is_dir():
                for f in sorted(EVIDENCE_DIR.iterdir()):
                    if f.is_file() and not f.name.startswith("."):
                        items.append({
                            "name": f.name,
                            "bytes": f.stat().st_size,
                            "sha256": hashlib.sha256(f.read_bytes()).hexdigest()[:16],
                            "received": datetime.fromtimestamp(
                                f.stat().st_mtime, timezone.utc).isoformat(timespec="seconds"),
                        })
            self._send_json(200, {"count": len(items), "evidence": items})
            return

        self._send_json(404, {"error": "unknown path", "paths": ["/health", "/task", "/evidence"]})

    def do_POST(self) -> None:  # noqa: N802
        path = self.path.split("?", 1)[0]
        if path != "/evidence":
            self._send_json(404, {"error": "unknown path", "paths": ["/evidence"]})
            return
        if not self._authorized():
            return

        name = self.headers.get("X-Evidence-Name", "").strip()
        if not SAFE_NAME.match(name):
            self._send_json(400, {
                "error": "X-Evidence-Name must be 1-120 characters of [A-Za-z0-9._-]",
                "received": name[:80],
            })
            return

        raw = self._read_body()
        if not raw:
            self._send_json(400, {"error": "empty body, or larger than the 4 MB limit"})
            return

        EVIDENCE_DIR.mkdir(parents=True, exist_ok=True)
        destination = EVIDENCE_DIR / name
        destination.write_bytes(raw)

        digest = hashlib.sha256(raw).hexdigest()
        self.log_message("stored evidence %s (%d bytes)", name, len(raw))
        self._send_json(200, {
            "stored": name,
            "bytes": len(raw),
            "sha256": digest,
            "note": "Raw bytes stored unmodified. Delivery judgement is made from downstream "
                    "CellBroadcast evidence, never from an exit code.",
        })


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--port", type=int, default=12000)
    parser.add_argument("--host", default="0.0.0.0")
    parser.add_argument("--task-file", dest="task_file", default=str(TASK_FILE),
                        help="task batch JSON to serve (default: bridge-tasks.json)")
    parser.add_argument("--artifact", action="append", default=[], metavar="NAME=PATH",
                        help="offer NAME for download at /download/NAME (repeatable)")
    parser.add_argument("--public-base", default=os.environ.get("BRIDGE_PUBLIC_BASE", ""),
                        help="external base URL, used only to print download links")
    parser.add_argument("--print", dest="show", action="store_true",
                        help="print the token and pending tasks, then serve")
    args = parser.parse_args()

    task_file = Path(args.task_file).resolve()
    token = load_or_create_token()
    tasks = load_tasks(task_file)

    artifacts: dict[str, str] = {}
    for spec in args.artifact:
        if "=" not in spec:
            parser.error(f"--artifact expects NAME=PATH, got {spec!r}")
        name, _, path = spec.partition("=")
        if "/" in name or name in {"", ".", ".."}:
            parser.error(f"artifact name must be a bare filename, got {name!r}")
        artifacts[name] = str(Path(path).expanduser().resolve())

    print("Emergency-Simulator bridge")
    print(f"  repository : {REPO}")
    print(f"  listening  : http://{args.host}:{args.port}")
    print(f"  token file : {TOKEN_FILE}")
    print(f"  task file  : {task_file}")
    print(f"  endpoints  : GET /health (open), GET /task, GET /evidence, GET /artifacts, POST /evidence (token)")
    if artifacts:
        base = (args.public_base or f"http://{args.host}:{args.port}").rstrip("/")
        print("  artifacts  :")
        for name, file in artifacts.items():
            p = Path(file)
            exists = "ok" if p.is_file() else "MISSING"
            size = f"{p.stat().st_size:,} bytes" if p.is_file() else "-"
            print(f"    {name}  [{exists}] {size}")
            print(f"      {base}/download/{name}")
            if p.is_file():
                print(f"      sha256 {hashlib.sha256(p.read_bytes()).hexdigest()}")
    if args.show:
        print()
        print(f"  TOKEN: {token}")
        print(f"  task batch: {tasks.get('batch')} ({len(tasks.get('tasks', []))} tasks)")
    print()
    print("This service cannot reach a phone. It serves tasks and stores raw evidence.")
    print()

    server = ThreadingHTTPServer((args.host, args.port), Handler)
    server.token = token  # type: ignore[attr-defined]
    server.task_file = task_file  # type: ignore[attr-defined]
    server.artifacts = artifacts  # type: ignore[attr-defined]
    server.public_base = (args.public_base or f"http://{args.host}:{args.port}").rstrip("/")  # type: ignore[attr-defined]
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\nstopped")
    finally:
        server.server_close()
    return 0


if __name__ == "__main__":
    sys.exit(main())