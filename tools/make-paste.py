#!/usr/bin/env python3
"""Render the operator's one-paste block with the *current* host and token.

Why this exists
---------------

``docs/connect-one-paste.md`` is committed and the repository is public, so it can only ever carry
placeholders. The bearer token is generated per session and lives in ``bridge-token.txt``
(gitignored). This script joins the two: it reads the template, substitutes the live values, and
writes the result to a gitignored file.

The failure this prevents is specific and was observed: a previous session committed a live token
into the template, the host later died, and the stale values were handed to the operator. The paste
looked correct and silently failed at step 1. Deriving the values here, on demand, makes it
impossible to ship a dead host or a stale token without also shipping a stale token file.

Usage
-----

    python3 tools/make-paste.py --host https://<your-host>          # write + print
    python3 tools/make-paste.py --host https://<your-host> --stdout # print only

The output file is ``docs/connect-one-paste.local.md`` and is gitignored.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
TEMPLATE = REPO / "docs" / "connect-one-paste.md"
TOKEN_FILE = REPO / "bridge-token.txt"
OUTPUT = REPO / "docs" / "connect-one-paste.local.md"

#: The rendered config block. Everything between these markers is replaced wholesale, so the
#: committed template can explain the placeholder situation without that wording surviving into the
#: rendered paste, where it would contradict the filled-in values.
BEGIN_MARK = "# @@BEGIN-CONFIG@@\n"
END_MARK = "# @@END-CONFIG@@\n"


def load_token() -> str:
    if not TOKEN_FILE.is_file():
        sys.exit(
            f"no token at {TOKEN_FILE}\n"
            "Start the bridge first: python3 tools/bridge.py --port 12000"
        )
    token = TOKEN_FILE.read_text(encoding="utf-8").strip()
    if not token:
        sys.exit(f"token file {TOKEN_FILE} is empty; restart the bridge")
    return token


def render(host: str, token: str) -> str:
    text = TEMPLATE.read_text(encoding="utf-8")

    start = text.find(BEGIN_MARK)
    end = text.find(END_MARK)
    if start == -1 or end == -1 or end < start:
        sys.exit(
            f"template {TEMPLATE} no longer contains the {BEGIN_MARK.strip()} / {END_MARK.strip()}\n"
            "markers around the $base and $token lines. Restore them, then re-run this generator."
        )

    host = host.rstrip("/")
    block = (
        f"{BEGIN_MARK}"
        f'$base  = "{host}"\n'
        f'$token = "{token}"\n'
        f"{END_MARK}"
    )
    # ``end`` is the start of END_MARK; replace through the end of that line.
    return text[:start] + block + text[end + len(END_MARK):]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--host", required=True,
                        help="the analysis host for THIS session, e.g. https://work-1-....all-hands.dev")
    parser.add_argument("--stdout", action="store_true",
                        help="print the rendered paste instead of writing the local file")
    args = parser.parse_args()

    token = load_token()
    rendered = render(args.host, token)

    if args.stdout:
        print(rendered)
        return 0

    OUTPUT.write_text(rendered, encoding="utf-8")
    print(f"wrote {OUTPUT}")
    print(f"  host  : {args.host.rstrip('/')}")
    print(f"  token : {token[:6]}...{token[-4:]}  ({len(token)} chars)")
    print()
    print("Send the operator the ```powershell block from that file. The token is not committed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())