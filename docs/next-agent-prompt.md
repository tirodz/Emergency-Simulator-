# Mission 3 — starting prompt for a new session

Paste this entire document as the opening message of a new conversation. It is written to be
self-contained: it assumes you remember nothing.

---

## What you are continuing

**Project:** Emergency-Simulator
**Repository:** `https://github.com/tirodz/Emergency-Simulator-`
**Active branch:** `release/self-contained-windows-build` (head `224624f`)
**Open PR:** `#1` — **draft. Do not merge without clearing the release gate at the bottom.**

Read these before doing anything else, in this order:

1. `progress.md` — the live state. Authoritative over the PR description.
2. `report.md` — the full feasibility investigation.
3. `docs/experiments/` — every experiment with raw evidence.
4. `docs/bugs/` — ten documented defects, each with the test that guards it.
5. `docs/next-agent-prompt.md` — this document, kept in the repo so it survives.

---

## The one question this mission must answer

> Is there **any legitimate, non-root** path from a Windows PC over Wi-Fi/IP that makes a
> **completely stock Samsung Galaxy A35** process a test alert through its **genuine
> `CellBroadcastReceiver`**?

Nobody has answered this yet. Everything else is secondary to it.

---

## What is already proven — do not re-litigate, do not weaken

Under **controlled privileged conditions** on a rooted Android 15 / API 35 **userdebug emulator**, the
real Android alert pipeline is proven end to end:

```
Windows GUI -> adb -> injected SmsCbMessage -> android.provider.action.SMS_EMERGENCY_CB_RECEIVED
  -> genuine CellBroadcastReceiver -> genuine CellBroadcastAlertService
  -> genuine alert audio -> genuine CellBroadcastAlertDialog
```

Real, evidence-backed, and **it says nothing about a stock retail phone.** Do not present the two as
one claim. That conflation is the specific error this project exists to avoid.

## What is NOT established

* Whether a stock A35 can be triggered at all without root. **This is the mission.**
* `EXP-ALERT-002` — lock screen presentation, vibration, DND override. Unverified. The emulator may
  not be able to demonstrate vibration at all; if so write `UNVERIFIED — emulator limitation`.
* A Windows end-to-end run against a real device. CI has no device, so the Windows adb transport has
  never been exercised end to end.

---

## The environment constraint that decides everything

**You are in a Linux container.** It has:

* no USB;
* no access to the operator's Windows filesystem;
* **no route outward to the operator's laptop** — every network path is inbound-only;
* no `adb`, no Android SDK, no emulator preinstalled;
* no KVM, so an AVD runs under software emulation and takes ~9 minutes to cold boot.

The Galaxy A35 is on the operator's USB. **You cannot see it.** Do not write commands that assume you
can run `adb` locally against their phone. They will fail and burn the run.

### The only working topology

```
   OPERATOR'S WINDOWS LAPTOP  --adb-->  Galaxy A35 (stock, read-only)
              |
              +--HTTPS (outbound from the laptop)-->  you
```

The laptop is the bridge. You author task batches; the laptop runs them and posts raw output back.

### CRITICAL: the previous session's URL and token are dead

The previous container died. Its host and token are recorded in `docs/connect-one-paste.md` and
`docs/bridge-setup.md` and **must not be reused** — they will not work.

**You must derive your own.** Your session context lists the hosts available for this conversation
(look for a `work_hosts` block with URLs on ports 12000 and 12001). Use **your** host, not the old one.

1. Start the bridge:
   `cd /workspace/project/Emergency-Simulator- && python3 tools/bridge.py --port 12000 &`
2. Read your fresh token: `cat bridge-token.txt`
3. **Verify the port is genuinely reachable from outside**, using an external fetcher rather than a
   self-check that can hairpin back to itself:
   `curl -s "https://r.jina.ai/<YOUR_HOST>/health"`
   You want `"ok": true`. Do not tell the operator the connection works until this passes.
4. Regenerate the paste with your host and token, then give the operator the whole block in chat.

---

## Infrastructure that already exists (all committed and pushed)

| Path | What it is |
| --- | --- |
| `tools/bridge.py` | Serves a task batch, stores posted evidence. Token-gated. Cannot touch a phone; no path from an inbound request to code execution. |
| `bridge-tasks.json` | The batch it serves. Currently **A35-RO-001**: ten read-only probes. |
| `docs/connect.ps1` | The PowerShell the operator runs. |
| `docs/connect-one-paste.md` | Generated paste doc — **contains a dead URL and token; regenerate it.** |
| `docs/bridge-setup.md` | Topology explanation — also contains the dead values. |
| `evidence/` | Where posted raw output lands (gitignored, currently empty). |

Bridge endpoints: `GET /health` (open), `GET /task`, `GET /evidence`, `POST /evidence` (token).

The batch's filters are already PowerShell-native `Select-String`. Do not reintroduce `findstr`, whose
`/I -A4` options are grep syntax and fail in PowerShell.

---

## Your immediate objective

Ask the operator to paste the connection block. Expect ten evidence files at `GET /evidence`. When
they arrive:

1. **Read the raw evidence.** The answers are in the `dumpsys package` output. Do not skim it.
2. Answer, with evidence:
   * Which package owns alerts on this A35 — AOSP `com.android.cellbroadcastreceiver`, a Samsung one,
     or both?
   * Is the emergency permission `signature|privileged`? **That single fact decides whether an ordinary
     app can ever trigger the broadcast.**
   * Are any components exported in a way an external caller could legitimately reach?
   * Does a legitimate test component exist at all, as AOSP ships one?
   * Is the module an APEX, and which version?
   * What is the real carrier (CSC, `gsm.operator.numeric`)? Needed for the network question.
3. Write findings to `docs/stock-device/aosp-vs-samsung.md` and
   `docs/stock-device/security-boundary.md`. Label every claim
   **CONFIRMED / LIKELY / INFERRED / UNKNOWN / BLOCKED / NOT APPLICABLE**. Never upgrade INFERRED to
   CONFIRMED without evidence.
4. If a legitimate non-privileged entry point exists, author a **second batch** that attempts it — and
   get the operator's explicit approval for that specific command first, because it crosses from
   inspection into an attempt.
5. If none exists, **that is the answer.** Document the boundary and stop. Do not hunt for a way around
   it.

Also document the router/network boundary (Phase 5): a consumer router carries IP traffic, and does
**not** become a Cell Broadcast Centre, base station, or modem. Establish whether any legitimate
operator-side test interface exists. Do not pretend one does.

---

## Hard rules — not negotiable

**The phone.** It is the operator's private property, not a disposable device.
* **Never** root it, unlock the bootloader, flash it, install a custom recovery or ROM, modify firmware,
  remount a partition, install a privileged APK, or replace an Android/Samsung component.
* **Read-only first.** Before any command ask: *does this modify the phone?* If yes, do not run it
  without the operator's explicit approval for that specific command, and only if it is reversible.
* Prefer effects that are temporary and confined to the test process.
* **A blocked test is a valid, successful result.** "Blocked by Samsung permissions" and "the interface
  does not exist on stock firmware" are answers, not failures to route around.
* If you find something undocumented or apparently vulnerable: **STOP at characterization.** Document
  component, interface, required permissions, observed behaviour. Do not weaponize it, bypass a
  signature permission, defeat SELinux, or attack anything.

**Transmission.** No cellular transmission, ever. No RF, no carrier CBC impersonation, no operator
network injection, no SDR or base-station hardware.

**Evidence.**
* **Never judge delivery from an exit code** — not adb's, not a process's, not "no exception thrown".
  Two of the ten recorded bugs were *false successes* where every signal said the work had succeeded
  and it had not.
* Delivery is established only from downstream evidence: `CellBroadcastReceiver` →
  `CellBroadcastAlertService` → `CellBroadcastAlertAudio` → `CellBroadcastAlertDialog` in logcat.
* If that evidence is absent, report **UNKNOWN / UNCERTAIN**, never success.
* ADB is transport, not privilege escalation. "The command was accepted" ≠ "Android authorised it".
  That distinction *is* the investigation.

**Honesty.**
* Never claim the stock A35 works if it has not been demonstrated.
* Never expose a fake "stock mode"; never silently fall back to a custom notification.
* If you cannot test something, write `UNVERIFIED` and say why. Do not convert missing evidence into a
  claimed success.
* The README and GUI must not imply "works on any Android phone". They must distinguish
  **CONTROLLED DEVELOPMENT MODE** (rooted/userdebug, adb) from **STOCK DEVICE MODE** (only if actually
  demonstrated).

---

## Working style

* **Incremental.** Never attempt the whole mission in one run. Checkpoint often — the previous sandbox
  died mid-session.
* Assume the sandbox can die at any moment: commit and push after every meaningful step.
* Update `progress.md` after every milestone. Never leave it describing an outdated state.
* Small coherent commits, human-readable messages (`docs:`, `research:`, `fix:`). No AI or bot
  attribution, no co-author trailers, no fabricated contributors. Use the repo's existing Git identity.

---

## Release gate — do not merge PR #1 until all hold

* [ ] `EXP-ALERT-002` documented
* [ ] Windows EXE exercised against a real device
* [ ] Physical-device result documented
* [ ] Stock Galaxy A35 compatibility investigated
* [ ] AOSP vs Samsung comparison documented
* [ ] Router/network boundary documented
* [ ] Security boundary documented
* [ ] `README.md` accurately describes current capabilities
* [ ] No false claim that stock A35 support exists
* [ ] No fake alert fallback
* [ ] No unauthorized cellular/RF/network injection
* [ ] All tests green
* [ ] All changes committed and pushed

CI being green is **not** sufficient. The gate is about truthfulness, not the build.

---

## Your first actions, right now

1. `git log --oneline -6` and `git status` — orient. Confirm head is `224624f` or later and the tree
   is clean.
2. Read `progress.md`.
3. Start the bridge on port 12000, read your fresh token, and **verify reachability with an external
   fetcher**.
4. `GET /evidence` — has anything been posted? Almost certainly not; the previous sandbox died with an
   empty evidence store.
5. Greet the operator, regenerate the connection paste with **your** host and token, and give them the
   entire block to paste into PowerShell.
6. Wait. Do not start building features.

---

## State of the world at handoff

* All code and docs are **committed and pushed** on `release/self-contained-windows-build`. Nothing was
  lost when the previous sandbox died.
* `evidence/` is **empty** — no A35 data has ever been collected.
* The A35 has **never been touched** by this project. No command has been run against it.
* PR #1 is a **draft**, CI green, deliberately not merged.
* Last commit: `224624f feat: give the operator one paste to connect, and a brief for the next agent`.
