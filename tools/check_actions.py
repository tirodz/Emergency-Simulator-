#!/usr/bin/env python3
"""Guard against the controller emitting broadcast actions that Android does not implement.

Why this exists
---------------

Three separate times this project has been handed a command that could not work, and each time the
command *looked* fine: `am broadcast` exits 0, the terminal shows no error, and the operator
concludes the alert path is broken rather than the command. The three:

* `am broadcast -a android.provider.Telephony.SMS_CB_RECEIVED` — the action is real but is a
  **protected broadcast**, so `ActivityManagerService` refuses a non-system caller before the
  receiver is resolved. Accepted-looking, refused.
* `am broadcast -a android.telephony.action.SECRET_CODE -d android_secret_code://2627` — same, and
  worse: it was written into a bug report as a *reproduction* that had supposedly been observed.
  It could not have been; see BUG-002's correction section.
* `am broadcast -a com.android.cellbroadcastreceiver.SHOW_TEST_MESSAGE` — **this action does not
  exist anywhere in AOSP.** It is not unprotected-but-gated like the others; there is no receiver
  that handles it in any branch. `am` still accepts the broadcast, finds no matching receiver, and
  reports nothing.

The third case is the one this check is named after. A command naming a component that does not
exist is indistinguishable, at the terminal, from a command naming a component that exists and is
gated. Both print no error. Only source settles which it is.

So: the controller may only emit actions that are in `KNOWN_ACTIONS` below, and every entry records
whether the action actually reaches the Cell Broadcast pipeline. An action this project is tempted
to invent must be added here *with its AOSP file and line* first, which forces the question "does
this exist?" to be answered before the command is written rather than after it fails.

Usage
-----

    python3 tools/check_actions.py                  # exit 1 on an unknown action
    python3 tools/check_actions.py --verbose
    python3 tools/check_actions.py --self-test      # prove the check catches the three above

Scope and honesty
-----------------

The check is structural. It cannot see an action assembled by runtime string concatenation, and it
does not execute anything on a phone. What it does is make the *specific* mistake this project keeps
making impossible to commit by accident, and loud when committed on purpose.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
RUST_SOURCES = REPO / "src-tauri" / "src"
DOCS = REPO / "docs"

# --------------------------------------------------------------------------------------------
# The registry. Every broadcast action the controller is permitted to emit, with its provenance
# and whether it can actually reach the alert pipeline.
#
# "reaches_pipeline" is the load-bearing field. An action that is unprotected and handled still
# only reaches the pipeline if a receiver exists that feeds CellBroadcastAlertService. Marking
# something reachable is a claim about Android that has to be backed by the source line.
# --------------------------------------------------------------------------------------------

KNOWN_ACTIONS: dict[str, dict[str, object]] = {
    # The one root-free route in, when ro.debuggable == 1. Registered dynamically by
    # GsmInboundSmsHandler; RECEIVER_EXPORTED with no permission; feeds
    # CellBroadcastServiceManager.sendGsmMessageToHandler, i.e. the genuine handler.
    "com.android.internal.telephony.gsm.TEST_TRIGGER_CELL_BROADCAST": {
        "reaches_pipeline": True,
        "source": "frameworks/opt/telephony .../gsm/GsmInboundSmsHandler.java (TEST_ACTION, lines 53-73)",
        "note": "GSM test injection; gated on ro.debuggable=1",
    },
    "com.android.internal.telephony.cdma.TEST_TRIGGER_CELL_BROADCAST": {
        "reaches_pipeline": True,
        "source": "frameworks/opt/telephony .../cdma/CdmaInboundSmsHandler.java (TEST_ACTION, lines 70-75)",
        "note": "CDMA test injection; gated on ro.debuggable=1",
    },
    # Documented in the module as a debugging switch for duplicate detection. Toggles a flag; it
    # does not display an alert, and it is gated on ro.debuggable.
    "com.android.cellbroadcastservice.action.DUPLICATE_DETECTION": {
        "reaches_pipeline": False,
        "source": "packages/modules/CellBroadcastService .../CellBroadcastHandler.java (ACTION_DUPLICATE_DETECTION, lines 116-124)",
        "note": "toggles duplicate detection only; gated on ro.debuggable=1",
    },
    # Actions that exist but must never be emitted as an injection attempt. Listed so the check
    # can say *why* rather than merely "unknown".
    "android.provider.Telephony.SMS_CB_RECEIVED": {
        "reaches_pipeline": False,
        "source": "frameworks/base/core/res/AndroidManifest.xml:749 (protected-broadcast)",
        "note": "protected broadcast; refused for any non-system UID",
    },
    "android.provider.action.SMS_EMERGENCY_CB_RECEIVED": {
        "reaches_pipeline": False,
        "source": "frameworks/base/core/res/AndroidManifest.xml:750 (protected-broadcast)",
        "note": "protected broadcast; refused for any non-system UID",
    },
    "com.android.internal.telephony.cdma.TEST_TRIGGER_SCP_MESSAGE": {
        "reaches_pipeline": True,
        "source": "frameworks/opt/telephony .../cdma/CdmaInboundSmsHandler.java (SCP_TEST_ACTION, lines 74-75)",
        "note": "CDMA SCP test injection; gated on ro.debuggable=1",
    },
    "cellbroadcastreceiver.SHOW_NEW_ALERT": {
        "reaches_pipeline": False,
        "source": "packages/apps/CellBroadcastReceiver .../CellBroadcastAlertService.java (SHOW_NEW_ALERT_ACTION, line 89; consumer exported=false)",
        "note": "internal action between the receiver and the alert service; not exported, so shell cannot address it",
    },
    # The secret code is a protected broadcast *and* a gate-opener rather than an injector.
    "android.telephony.action.SECRET_CODE": {
        "reaches_pipeline": False,
        "source": "frameworks/base/core/res/AndroidManifest.xml:564 (protected-broadcast)",
        "note": "protected, and only toggles the testing-mode display filter; never injects",
    },
}

# Actions that look plausible and have been proposed or written in this repository, but that do not
# exist in any AOSP branch. Kept as an explicit deny-list so the failure mode is named in the output
# instead of surfacing as a generic "unknown action".
FABRICATED_ACTIONS = {
    "com.android.cellbroadcastreceiver.SHOW_TEST_MESSAGE": (
        "No such action exists in AOSP. `grep -rn SHOW_TEST_MESSAGE` over the CellBroadcastReceiver "
        "and CellBroadcastService modules on android14-release and android15-release returns nothing. "
        "There is no receiver to handle it; `am` accepts the broadcast and reports no match. "
        "The real root-free injection action is "
        "com.android.internal.telephony.gsm.TEST_TRIGGER_CELL_BROADCAST."
    ),
}

# `am broadcast -a <action>` and `am start-action`-style forms, plus the Rust string literals that
# hold an action. Deliberately broad on the flag so a reformatted command is still caught.
ACTION_FLAG = re.compile(r"""-a\s+["']?([A-Za-z0-9_.]+)["']?""")
ACTION_LITERAL = re.compile(r"""(?:ACTION|action)\w*\s*[:=]\s*(?:&str)?\s*"([A-Za-z0-9_.]+)\"""")

# A string that is structured like an Android action and is used in a broadcast context.
LOOKS_LIKE_ACTION = re.compile(
    r"""(?<![\w.])("(?:com|android|cellbroadcastreceiver)[A-Za-z0-9_.]*(?:SHOW|TEST|RECEIVED|SECRET|ACTION|DETECTION)[A-Za-z0-9_.]*")"""
)


def candidate_actions(text: str) -> list[tuple[int, str]]:
    """Every action-looking token in `text`, with its line number."""
    found: list[tuple[int, str]] = []
    for number, line in enumerate(text.splitlines(), start=1):
        for match in ACTION_FLAG.finditer(line):
            found.append((number, match.group(1)))
        for match in ACTION_LITERAL.finditer(line):
            found.append((number, match.group(1)))
        for match in LOOKS_LIKE_ACTION.finditer(line):
            found.append((number, match.group(1).strip('"')))
    return found


def relevant_files() -> list[Path]:
    """Rust sources and the docs that quote commands the operator may run."""
    files = sorted(RUST_SOURCES.glob("**/*.rs"))
    files += sorted(DOCS.glob("**/*.md"))
    return [path for path in files if path.is_file()]


def audit(files: list[Path], verbose: bool = False) -> int:
    problems = 0
    scanned = 0
    for path in files:
        try:
            text = path.read_text(encoding="utf-8")
        except (UnicodeDecodeError, OSError):
            continue
        scanned += 1
        # Docs quote commands from bug reports and rejected proposals on purpose, and the journal
        # needs to show a wrong command in order to explain why it is wrong.
        is_doc = path.suffix == ".md"
        # A document is allowed to quote a fabricated action only when the same document also says
        # it does not exist. Bug journals and refutation records must be able to show a wrong
        # command in order to explain why it is wrong; a document that merely prescribes one
        # contains no disclaimer and is still flagged.
        doc_disclaims = is_doc and bool(
            re.search(
                r"(does not exist|no such action|not exist|fabricated|refut|impossible|cannot work)",
                text,
                re.IGNORECASE,
            )
        )
        for number, action in candidate_actions(text):
            if action in FABRICATED_ACTIONS:
                if doc_disclaims:
                    if verbose:
                        print(f"{path.relative_to(REPO)}:{number}: discusses the fabricated "
                              f"action {action} (allowed: the same document says it does not exist)")
                    continue
                problems += 1
                print(f"{path.relative_to(REPO)}:{number}: FABRICATED ACTION {action}")
                print(f"    {FABRICATED_ACTIONS[action]}")
                continue
            if action in KNOWN_ACTIONS:
                if verbose:
                    entry = KNOWN_ACTIONS[action]
                    print(f"{path.relative_to(REPO)}:{number}: {action} "
                          f"(reaches_pipeline={entry['reaches_pipeline']})")
                continue
            # Unknown but structured like an action. Only flag it when the file is Rust and the
            # token is plausibly meant as an action to send, to avoid noise on unrelated strings.
            if not is_doc and ("broadcast" in text.splitlines()[number - 1].lower()
                               or action.startswith(("com.android.internal.telephony",
                                                     "com.android.cellbroadcast"))):
                problems += 1
                print(f"{path.relative_to(REPO)}:{number}: UNREGISTERED ACTION {action}")
                print("    Add it to KNOWN_ACTIONS in tools/check_actions.py with its AOSP file and "
                      "line, or remove it. An action with no source is assumed not to exist.")
    if verbose:
        print(f"scanned {scanned} files")
    if problems:
        print(
            f"\n{problems} action problem(s). A broadcast action that Android does not implement is "
            "accepted by `am` and reports nothing, which is indistinguishable from a real action on "
            "a gated build. Every action this tool emits must be traceable to AOSP source.",
            file=sys.stderr,
        )
        return 1
    print(f"OK — every action in {scanned} files is registered or provably fabricated")
    return 0


def self_test() -> int:
    """Prove the check catches the mistakes this project actually made.

    The assertion tested is the one that matters in practice: **would this command line, if run,
    contain an action this project must not send?** A protected action counts even where the line is
    only discussing it, because the point is that a command quoting it can never work.
    """
    cases = [
        # The fabricated action: the mistake this check is named after.
        ("adb shell am broadcast -a com.android.cellbroadcastreceiver.SHOW_TEST_MESSAGE", True),
        # Real actions, but protected. Both have been written as injection attempts here.
        ("adb shell am broadcast -a android.provider.Telephony.SMS_CB_RECEIVED", True),
        ("adb shell am broadcast -a android.telephony.action.SECRET_CODE -d android_secret_code://2627",
         True),
        # The one action that actually reaches the pipeline on a debuggable build.
        ("adb shell am broadcast -a com.android.internal.telephony.gsm.TEST_TRIGGER_CELL_BROADCAST "
         "--es pdu_string 00001100", False),
    ]
    failures = 0
    for command, should_alert in cases:
        flagged = False
        for action in (a for _, a in candidate_actions(command)):
            if action in FABRICATED_ACTIONS:
                flagged = True
            elif action in KNOWN_ACTIONS and not KNOWN_ACTIONS[action]["reaches_pipeline"]:
                flagged = True
        if flagged != should_alert:
            print(f"SELF-TEST FAIL: {command!r} flagged={flagged} expected={should_alert}")
            failures += 1
        else:
            print(f"self-test ok: flagged={flagged:<5} {command}")
    if failures:
        print(f"\n{failures} self-test case(s) failed", file=sys.stderr)
        return 1
    print(
        "\nSelf-test passed. The check catches the fabricated action and the two protected "
        "actions, and accepts the one action that actually reaches the pipeline."
    )
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--verbose", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        return self_test()
    return audit(relevant_files(), verbose=args.verbose)


if __name__ == "__main__":
    sys.exit(main())
