# Progress

> This file is the live state of the project. A future session must be able to continue from here
> without reading the whole repository. Never leave it describing an outdated state.

## Current status

**Phase 1 (research / feasibility) — COMPLETE for the source-analysis portion.**
No implementation has begun, by design. The deliverable of this phase is the documentation set under
`docs/` plus `report.md`, which together answer the central feasibility question with cited evidence.

The central question:

> Can a PC or controller phone legitimately cause an Android device to process a controlled Cell
> Broadcast test message through Android's *genuine* emergency-alert subsystem?

**Answer: yes — via the mechanism AOSP itself provides, on a device we control at the system level.
It is not possible on a stock, unrooted retail device, and not possible over ADB alone.**

## What has been established

* The genuine pipeline and its exact component boundaries (source-verified).
* The AOSP test application's identity, permissions, UID, signing and exact call path.
* Where the test path enters the real pipeline: at the ordered-broadcast boundary between
  `CellBroadcastService` (CBS) and `CellBroadcastReceiver` (CBR).
* The four security gates that block a normal app, and which one is load-bearing.
* The channel-configuration filters (`testing_mode`, `debug_build`, `display`) and the user-toggle
  defaults, including the important fact that a channel with no configured range is silently dropped.
* The ETWS-vs-CMAS test mechanisms and which categories are safe to use.
* That no "rocket attack" alert class exists in Android, and that the visible body text is free-form.
* That the genuine alert UI, sound and vibration depend on nothing from the modem.

## Completed

* [x] Repository inspection: branch `main`, clean tree, only `README.md`, initial commit `89ec245`.
* [x] Cloned and inspected `packages/apps/CellBroadcastReceiver` on `main`, `android14-release`,
      `android15-release`, `android16-release`.
* [x] Cloned and inspected `packages/modules/CellBroadcastService` on `main`.
* [x] Fetched and inspected `frameworks/base` `AndroidManifest.xml`, `AppOpsManager.java`,
      `provider/Telephony.java`, `telephony/SmsCbMessage.java`, `SmsCbCmasInfo.java`,
      `ICellBroadcastService.aidl`, `services/core/.../BroadcastController.java`,
      `services/core/.../ActivityManagerService.java`, `app/LoadedApk.java`.
* [x] Fetched and inspected `frameworks/opt/telephony`
      `CellBroadcastServiceManager.java`.
* [x] Traced the AOSP test app: `SendTestMessages.java`, `SendGsmCmasMessages.java`,
      `SendCdmaCmasMessages.java`, `SendTestBroadcastActivity.java`, `GsmSmsCbMessage.java`.
* [x] Recorded every source with branch + commit SHA in `docs/sources.md`.
* [x] Wrote the full documentation set.

## Current milestone

Milestone: **"Research foundation"** — the documentation listed below, committed as one or more small
logical commits.

Deliverables produced:

| File | Status |
| --- | --- |
| `README.md` | done |
| `progress.md` | done (this file) |
| `report.md` | done |
| `docs/sources.md` | done |
| `docs/architecture.md` | done |
| `docs/android-cellbroadcast.md` | done |
| `docs/aosp-test-path.md` | done |
| `docs/privilege-model.md` | done |
| `docs/android-version-compatibility.md` | done |
| `docs/oem-compatibility.md` | done |
| `docs/transport-options.md` | done |
| `docs/protocol-and-alert-types.md` | done (added; covers the brief's §10/§11) |
| `docs/feasibility.md` | done |
| `docs/experiments.md` | done |
| `docs/security-and-safety.md` | done |
| `docs/open-questions.md` | done |
| `docs/critical-questions.md` | done (added; direct answers to all 68 questions from the brief) |

## Findings

### Confirmed facts (backed by AOSP source)

1. The emergency-alert experience — UI, sound, vibration, database — lives entirely inside the
   privileged `com.android.cellbroadcastreceiver` app, downstream of a broadcast.
2. The AOSP test app injects its message at that broadcast boundary and therefore exercises the
   genuine subsystem. It is signed with the **platform** key and shares **`android.uid.phone`**.
3. `ACTION_SMS_EMERGENCY_CB_RECEIVED` and `SMS_CB_RECEIVED` are `<protected-broadcast>` actions.
   `BroadcastController` throws `SecurityException` unless the caller's app id is `ROOT_UID`,
   `SYSTEM_UID`, `PHONE_UID`, `BLUETOOTH_UID`, `NFC_UID`, `SE_UID`, `NETWORK_STACK_UID`, or the caller
   is a persistent app. This check runs **before** any permission check.
4. `RECEIVE_EMERGENCY_BROADCAST` is `signature|privileged`; `BROADCAST_SMS` is `signature`;
   `RECEIVE_SMS` is `dangerous` + `hardRestricted`.
5. `CellBroadcastReceiver.onReceive` performs **no caller-identity check of its own**.
6. The receiver is a privileged mainline app (`privileged: true`, `certificate: "networkstack"` or
   `"platform"`, with a `privapp-permissions` allowlist).
7. ETWS has a first-class **test** warning type (`ETWS_WARNING_TYPE_TEST_MESSAGE = 0x03`, identifier
   `0x1103`). CMAS test-like classes are distinguished by identifier
   (`0x111C` monthly test, `0x111D` exercise, `0x111E` operator-defined).
8. Test toggles default to **off** (`test_alerts_enabled_default`,
   `test_exercise_alerts_enabled_default`, `test_operator_defined_alerts_enabled_default`,
   `state_local_test_alerts_enabled_default`).
9. CBR testing mode is toggled by `*#*#2627#*#*` and is permitted when `ro.debuggable == 1` **or**
   `allow_testing_mode_on_user_build` is true (true in AOSP defaults, **false** in the Japan/docomo
   overlay).
10. Channels marked `debug_build=true` are dropped unless `ro.debuggable == 1`.
11. **An alert on a channel with no configured range is silently dropped** — no UI, no sound, no
    database row (`range != null && range.mDisplay` is required).
12. The alert **body** is free-form (`SmsCbMessage.getMessageBody()`); the **title** is chosen by
    Android from the alert class and is overlayable by OEMs.
13. No "rocket attack"/"missile" identifier exists anywhere in Android's CB constants.
14. Nothing in the UI/sound/vibration path reads the modem; the genuine experience does not require a
    real broadcast.
15. No shell-command injection interface for CB exists in either CB module.

### Hypotheses (plausible, not yet verified)

* A rooted device can satisfy the remaining gates (AppOps, SELinux) and inject the broadcast.
  -> Experiment 7.
* `am start` of the AOSP test app's activity from ADB produces a genuine alert, because the activity
  then broadcasts *as the privileged app*. -> Experiment 6. **This is the highest-value unknown.**
* An AOSP userdebug/eng emulator can host the full pipeline. -> Experiment 3.
* An OEM that ships the AOSP receiver behaves identically to AOSP for the injection path. ->
  Experiments 10–14.

## Blockers

* **No physical Android device has been used yet.** All results so far are source-derived. The next
  phase cannot progress without a userdebug/eng-capable device (or an emulator, if Experiment 3
  succeeds).
* **The platform signing key problem.** Installing the AOSP test app requires the key the target build
  is signed with. A custom AOSP build provides it; a stock retail device does not. -> Experiment 4.
* **SELinux and AppOps are unexamined.** Both could still close the rooted path. -> Experiment 7.

## Next exact actions

Small, ordered, actionable:

1. **Experiment 1** on any available Android device: read-only inspection of the CB receiver, its
   privileges, and the resolved actions. Record raw output in `experiments.md`.
2. **Experiment 2** on the same device: enumerate the alert toggles and try `*#*#2627#*#*`.
3. **Experiment 3**: check whether an AOSP/GSI emulator contains the CB apex and receiver.
4. **Experiment 6** (**highest value**): try `adb shell am start` on
   `com.android.cellbroadcastreceiver.tests/.SendTestBroadcastActivity` on any device where the test
   app exists, and observe whether a genuine alert results.
5. **Experiment 4**, only once a userdebug/eng device is available: build and install
   `CellBroadcastReceiverTests` from the matching tree.
6. **Experiment 7** only after 4/5: establish the minimal privilege set for a custom helper.
7. Then, and only then, **Experiment 15**: the smallest ADB-driven controller.

Do **not** start the GUI, the Wi-Fi transport, or the multi-device fan-out before step 1–5 conclude.

## Last known working state

* Repository: branch `main`, working tree clean at the start of this phase.
* Environment: `/usr/local/bin/python` (Python 3.13), git 2.47.3.
* Git identity: `TIRO <68867160+tirodz@users.noreply.github.com>` (the repository's existing identity).
* No builds have been attempted. No Android device or emulator has been used. No tests have been run.
* AOSP reference clones live outside the repository (under `/tmp` in the session that produced this
  phase). They are **not** required to continue: every path, branch and commit SHA is recorded in
  `docs/sources.md`, and each file can be re-fetched with the documented gitiles URL pattern.
* Remote: `origin` = `tirodz/Emergency-Simulator-.git`.

## Git commit

* Base: `89ec245` — "Initial commit".
* This phase, small logical commits on `main`:

| Commit | Subject |
| --- | --- |
| `4cff9a4` | docs: establish project scope and documentation map |
| `93b859f` | research: document Android Cell Broadcast architecture |
| `f8fe8ba` | research: analyse the AOSP Cell Broadcast test application |
| `e847625` | docs: document the privilege and security boundary |
| `b2d9b54` | research: document alert types and the alert experience |
| `87af9da` | docs: compare Android versions and OEM divergence |
| `0f918a9` | docs: define transport options and safety boundaries |
| `09754a0` | docs: record feasibility findings, unknowns and experiment plan |
| `0ac265b` | docs: add progress record and feasibility report |
| `6c961f6` | docs: answer the project's 68 critical questions |

**Source-analysis phase complete. Ready for the first controlled experiment.**
