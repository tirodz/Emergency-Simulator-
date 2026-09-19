# Mission 3 — starting prompt for the next agent

Paste this whole document as the opening message of a new session. It is written to be self-contained:
it assumes the reader has no memory of any previous session.

---

## You are continuing the Emergency-Simulator project

**Repository:** `https://github.com/tirodz/Emergency-Simulator-`
**Active branch:** `release/self-contained-windows-build`
**Open PR:** `#1` (draft — **do not merge without clearing the release gate below**)

Read these first, in this order, before doing anything else:

1. `progress.md` — the live state. It is authoritative over anything in the PR description.
2. `report.md` — the full feasibility investigation.
3. `docs/experiments.md` and `docs/experiments/` — every experiment with raw evidence.
4. `docs/bugs/` — ten documented defects, each with the test that guards it.
5. `docs/connect-one-paste.md` — how the operator connects their laptop to you.

---

## What is already established (do not re-litigate, do not weaken)

Under **controlled, privileged** conditions on a rooted Android 15 / API 35 **userdebug emulator**, the
genuine Android alert pipeline is proven end to end:

```
Windows GUI -> adb -> injected SmsCbMessage -> android.provider.action.SMS_EMERGENCY_CB_RECEIVED
   -> genuine CellBroadcastReceiver -> genuine CellBroadcastAlertService
   -> genuine alert audio -> genuine CellBroadcastAlertDialog
```

This is real and evidence-backed. **It does not establish anything about a stock retail phone.**

## What is NOT established (this is the actual work)

The user's real phone is a **completely stock Samsung Galaxy A35** — not rooted, bootloader locked, and
it must stay that way. The central unanswered question is:

> Is there ANY legitimate, non-root path from a Windows PC over Wi-Fi/IP to make that stock A35's
> genuine `CellBroadcastReceiver` process a test alert?

Nobody has answered this yet. That is the mission.

---

## The environment constraint that determines everything

**You are in a Linux container.** It has:

* no USB;
* no access to the operator's Windows filesystem;
* **no route outward to the operator's laptop** — every network path is inbound-only;
* no `adb`, no Android SDK, no emulator preinstalled (you can install platform-tools from
  `https://dl.google.com/android/repository/` — egress works — but the emulator needs KVM, which this
  host lacks, so an AVD runs under software emulation and takes ~9 minutes to cold boot).

The Galaxy A35 is on the operator's USB. **You cannot see it.** Do not write commands that assume you
can run `adb` locally against their phone; they will fail and waste the run.

### The working topology

```
   OPERATOR'S WINDOWS LAPTOP  --adb-->  Galaxy A35 (stock, read-only)
              |
              +--HTTPS (outbound from the laptop)-->  you
```

The laptop is the bridge. You author task batches; the laptop runs them and posts raw output back.

### Tools that already exist for this

* `tools/bridge.py` — serves a task batch, stores posted evidence. Token-gated. Start it:
  `python3 tools/bridge.py --port 12000`. Token is in `bridge-token.txt` (gitignored).
* `bridge-tasks.json` — the task batch it serves. Currently **A35-RO-001**, ten read-only probes.
* `docs/connect.ps1` and `docs/connect-one-paste.md` — the PowerShell the operator pastes.
* `evidence/` — where posted raw output lands (gitignored).

The bridge's public URL is the `work-1-...prod-runtime.all-hands.dev` host in `docs/connect-one-paste.md`.
**Re-verify it is reachable** with an independent external fetch before telling the operator it works —
`curl -s "https://r.jina.ai/<url>/health"`. Do not trust a check that hairpins back to yourself.

---

## Your immediate objective

The operator is about to paste the connection script. Expect to receive ten evidence files. When they
arrive:

1. **Read the raw evidence.** Do not skim. The answers are in `dumpsys package` output.
2. Answer the Phase 4 questions from `docs/stock-device/`:
   * Which package owns alerts on this A35 — AOSP's `com.android.cellbroadcastreceiver`, a Samsung one,
     or both?
   * Is the emergency permission `signature|privileged`? That single fact decides whether an ordinary
     app can ever trigger the broadcast.
   * Are any components exported in a way an external caller could legitimately reach?
   * Does a legitimate test component exist at all, as AOSP ships one?
   * Is the module an APEX, and which version?
3. Write findings to `docs/stock-device/aosp-vs-samsung.md` and
   `docs/stock-device/security-boundary.md`, labelling every claim **CONFIRMED / LIKELY / INFERRED /
   UNKNOWN**. Never upgrade INFERRED to CONFIRMED without evidence.
4. If a legitimate non-privileged test entry point exists, author a **second batch** that attempts it —
   and get explicit operator approval for the specific command first, since that crosses from
   inspection into an attempt.
5. If none exists, **that is the answer.** Document the boundary and stop. Do not look for a way around
   it.

Also still open, and lower priority than the stock investigation:

* `EXP-ALERT-002` — lock screen, vibration, DND override. Unverified. The emulator may not be able to
  demonstrate vibration at all; if so, record `UNVERIFIED — emulator limitation`, never a success.
* A Windows end-to-end run against a real device. CI has no device, so the Windows adb transport has
  never been exercised end to end.

---

## Hard rules — these are not negotiable

**The phone:**
* It is the operator's private property, not a disposable development device.
* **Never** root it, unlock the bootloader, flash it, install a custom recovery or ROM, modify firmware,
  remount a partition, install a privileged APK, or replace an Android/Samsung component.
* **Read-only first.** Before any command, ask: *does this modify the phone?* If yes, do not run it
  unless the operator explicitly approves that specific command, and only if it is reversible.
* Prefer commands whose effects are temporary and confined to the testing process.
* A blocked test is a **valid, successful result**. "Blocked by Samsung permissions" and "the interface
  does not exist on stock firmware" are answers, not failures to route around.
* If you find something that looks undocumented or vulnerable: **STOP at characterization.** Document
  the component, interface, required permissions and observed behaviour. Do not weaponize it, bypass a
  signature permission, defeat SELinux, or attack anything.

**Transmission:**
* No cellular transmission, ever, by anyone. No RF, no carrier CBC impersonation, no operator network
  injection, no SDR or base-station hardware.
* A consumer router carries IP traffic. It does **not** become a Cell Broadcast Centre, base station, or
  modem. Document that clearly for Phase 5; do not pretend otherwise.

**Evidence:**
* **Never judge delivery from an exit code.** Not adb's, not the injector's, not a process's, not "no
  exception was thrown". Two of the ten recorded bugs were *false successes* where everything said the
  work had succeeded and it had not.
* Delivery is established only from downstream evidence: `CellBroadcastReceiver` →
  `CellBroadcastAlertService` → `CellBroadcastAlertAudio` → `CellBroadcastAlertDialog` in logcat.
* If that evidence is absent, report **UNKNOWN / UNCERTAIN**, not success.
* ADB is transport, not privilege escalation. "The command was accepted" ≠ "Android authorised it".
  That distinction *is* the investigation.

**Honesty:**
* Never claim the stock A35 works if it has not been demonstrated.
* Never expose a fake "stock mode", and never silently fall back to a custom notification.
* If you cannot test something, write `UNVERIFIED` and say why. Do not convert missing evidence into a
  claimed success.
* Do not confuse the rooted userdebug proof with the desired stock-phone product. They are different
  claims and must never be presented as one.

---

## Working style

* **Incremental.** Do not attempt the whole mission in one run. Checkpoint often.
* Update `progress.md` after every meaningful milestone — it is the memory across sessions. Never leave
  it describing an outdated state.
* Small, coherent commits with human-readable messages (`docs:`, `research:`, `fix:`). No AI or bot
  attribution, no co-author trailers, no fabricated contributors. Use the repository's existing Git
  identity.
* Record every confirmed fact immediately. Assume the next session remembers nothing.
* If a run is getting long, stop, write state, commit. A future agent must be able to continue.

---

## Release gate — do not merge PR #1 until all of these hold

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

CI being green is **not** sufficient. The gate is about truthfulness, not correctness of the build.

---

## First actions for you, right now

1. `git log --oneline -6` and `git status` — orient.
2. Read `progress.md`.
3. `python3 tools/bridge.py --port 12000 &` if the bridge is not already up, then re-verify public
   reachability with an external fetch.
4. Check `GET /evidence` — has the operator already posted the ten files? If yes, start at "Your
   immediate objective" step 1. If no, greet them, give them the paste from
   `docs/connect-one-paste.md`, and wait.

Do not start building features. The mission is to establish, with evidence, exactly where the boundary
is for a stock Samsung Galaxy A35 — and to say so plainly if the answer is that there is none.