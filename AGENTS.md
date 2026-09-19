# Working notes for agents in this repository

`progress.md` is the live state of the project and is authoritative over PR descriptions and these
notes. Read it first. `report.md` carries the full feasibility investigation, `docs/experiments/`
carries each experiment with its raw evidence, and `docs/bugs/` carries the defect journal.

## What this repository is

A study of whether a Windows PC can legitimately make an Android phone display a test emergency
alert, and a controller that does so under conditions it can actually verify. The distinction the
project exists to preserve:

* **CONTROLLED DEVELOPMENT MODE** — a rooted or `userdebug` device, driven over adb. Proven end to
  end: the real `CellBroadcastReceiver` → `CellBroadcastAlertService` → `CellBroadcastAlertDialog`
  chain produces a genuine alert.
* **STOCK DEVICE MODE** — an ordinary retail phone with no root. **Not demonstrated.** Do not write
  anything, in code or in `README.md`, that implies it works.

Never present the two as one claim. That conflation is the specific error the project is built to
avoid.

## Delivery is never judged from an exit code

`adb` exiting 0, a process not throwing, a command being accepted — none of these mean an alert
appeared. Two of the recorded bugs are false successes where every signal said the work had
succeeded and it had not (see `docs/bugs/`). Delivery is established only from downstream logcat
evidence. When that evidence is absent, report `UNKNOWN` or `UNCERTAIN`.

"ADB is transport, not privilege escalation." The command being accepted is not Android authorising
it, and that gap is the investigation rather than a detail.

## Hard constraints

* The operator's phones are private property. No rooting, bootloader unlock, flashing, custom
  recovery, ROM or firmware change, partition remount, privileged APK, or replacement of an
  Android/Samsung component. Read-only first; anything that could change phone state needs the
  operator's explicit approval for that specific command.
* No cellular transmission, ever: no RF, no carrier CBC impersonation, no operator network
  injection.
* If something undocumented or apparently vulnerable turns up, stop at characterisation — document
  the component, interface, required permissions and observed behaviour. Do not weaponise it.
* A blocked test is a valid result. "Blocked by Samsung permissions" and "the interface does not
  exist on stock firmware" are answers, not obstacles to route around.

## Credentials

This repository is **public**. Nothing session-specific may be committed: not a bearer token, not a
session host, not a rendered one-paste.

`tools/bridge.py` writes its token to `bridge-token.txt`, which is gitignored. The committed
`docs/connect-one-paste.md` holds `<HOST>` and `<TOKEN>` placeholders; `tools/make-paste.py` renders
the live values into `docs/connect-one-paste.local.md`, also gitignored. Hand the operator the
rendered block, never the placeholders, and never commit the render.

A previous session committed a live token to this public repository and then handed the operator a
dead host, which silently failed at the first step. Generate connection values fresh every session.

## Environment and topology

The analysis environment is a Linux container: no USB, no access to the operator's Windows
filesystem, no outbound route to their laptop. The operator's laptop is the bridge — it runs adb
against the phone and posts raw output back over HTTPS.

Start the bridge with `python3 tools/bridge.py --port 12000` and confirm reachability with an
**external** fetcher (`https://r.jina.ai/<HOST>/health`), not a self-check that can hairpin back to
itself. Do not tell the operator the connection works until that passes.

## House style

* Small coherent commits with human-readable messages (`docs:`, `research:`, `fix:`). No AI or bot
  attribution and no co-author trailers.
* Update `progress.md` after every milestone. Never leave it describing an outdated state.
* Commit and push after each meaningful step; the sandbox can die at any moment.
* Documented claims carry a label — `CONFIRMED`, `LIKELY`, `INFERRED`, `UNKNOWN`, `BLOCKED`,
  `NOT APPLICABLE`. Never upgrade `INFERRED` to `CONFIRMED` without new evidence.