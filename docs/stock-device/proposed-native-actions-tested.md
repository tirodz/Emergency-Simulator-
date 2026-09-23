# The proposed native trigger sequence, tested against AOSP

A task brief proposed a four-step native injection engine for this project:

1. `adb shell settings put global cell_broadcast_test_alerts 1`
2. `adb shell settings put global show_option_to_opt_out_notifications 1`
3. `adb shell am broadcast -a com.android.cellbroadcastreceiver.SHOW_TEST_MESSAGE`
4. `adb shell am start -n com.android.cellbroadcastreceiver/.CellBroadcastListActivity`

This document records what each one actually does, because four of the five omit a detail that makes
them fail silently — the failure shape this project exists to catch. It is written so no future
session has to re-derive it.

Every claim below is `CONFIRMED` against AOSP source read on `android14-release` and
`android15-release`, or `UNKNOWN` about Samsung specifically, stated as such.

---

## Verdict table

| # | Command | Verdict | Why |
|---|---|---|---|
| 1 | `settings put global cell_broadcast_test_alerts 1` | **no effect** | the key does not exist; per-SIM `SharedPreferences`, not a global setting |
| 2 | `settings put global show_option_to_opt_out_notifications 1` | **no effect** | same |
| 3 | `am broadcast -a ...cellbroadcastreceiver.SHOW_TEST_MESSAGE` | **does not exist** | no such action in any branch; nothing handles it |
| 4 | `am start -n .../.CellBroadcastListActivity` | **launches, injects nothing** | it is the message-history list; correct as a verification aid |
| 5 | `am broadcast -a android.provider.Telephony.SMS_CB_RECEIVED` | **refused** | protected broadcast |

### 1 and 2 — the settings keys do not exist

`grep -rn "cell_broadcast_test_alerts\|show_option_to_opt_out"` over both modules on
`android14-release` and `android15-release` returns nothing, and neither key appears in
`frameworks/base/core/res/AndroidManifest.xml`.

More fundamentally, the settings they aim to change are **not global settings at all**. They are
per-SIM `SharedPreferences`, read through `PreferenceManager.getDefaultSharedPreferences(context)`,
where the context is created per subscription. The alert preferences have keys like
`enable_test_alerts`, `enable_cmas_amber_alerts`, `enable_state_local_test_alerts`
(`CellBroadcastSettings.java` lines 107–137). There is no `Settings.Global` write that reaches them.

Writing a `settings put global` key that no code reads is the purest form of this project's
recurring failure: the command exits 0, the setting appears in `settings list global`, and nothing
changes. Worse, it *persists* — a future session reads it back, sees `1`, and believes test alerts
are enabled.

What the intent is reaching for does exist, but not as a shell command. The 2627 secret code opens
the testing-mode display filter, and it is triggered through a dialer — not by `settings put`. See
`A35-native-test-path.md` §4.

### 3 — `SHOW_TEST_MESSAGE` is fabricated

This is the most important row. `grep -rn "SHOW_TEST_MESSAGE"` over `packages/apps/CellBroadcastReceiver`
and `packages/modules/CellBroadcastService` on both branches returns **nothing**. There is no receiver
declared for it and none registered dynamically.

This is categorically different from the protected-broadcast problem. `SMS_CB_RECEIVED` exists and is
refused; `SHOW_TEST_MESSAGE` does not exist, so `am` resolves no receiver and reports nothing. At a
terminal the two are indistinguishable — both print no error. Only source separates them, which is
why `tools/check_actions.py` now exists.

The real action, and the only one that reaches the pipeline, is:

```
adb shell am broadcast -a com.android.internal.telephony.gsm.TEST_TRIGGER_CELL_BROADCAST \
  --es pdu_string <hex CB PDU>
```

It is documented by AOSP itself in `GsmInboundSmsHandler`'s comment and gated on `ro.debuggable == 1`.

### 4 — the activity launch is fine, but it is not an injection

`CellBroadcastListActivity` is exported and launches without a permission. It is the message-history
list. It does not and cannot originate a message. It belongs in the tool as a **verification step** —
"open the list and see whether a row appeared" is exactly the downstream evidence this project
demands — and it is already the right package-name source. The brief is also right that the package
name varies by OEM: AOSP uses `com.android.cellbroadcastreceiver`, and Samsung ships
`com.samsung.android.cellbroadcastreceiver` (`UNKNOWN` for this specific A35 until task 3 runs). The
controller already enumerates and ranks candidates for this reason (BUG-016).

### 5 — the protected broadcast, included because it is the natural next attempt

`android.provider.Telephony.SMS_CB_RECEIVED` is a protected broadcast
(`AndroidManifest.xml:749`). `ActivityManagerService` refuses any caller that is not a system UID, and
`SHELL_UID = 2000` is not one. `am broadcast ... -a com.android.cellbroadcastreceiver...` naming the
fabricated action is the same dead end.

---

## What was implemented instead

The brief's instructions were declined, because carrying them out would have shipped four
inoperative commands into a tool whose entire purpose is not to report success it has not observed.
What was implemented is the structural fix for the *reason* the brief looks plausible:

**`tools/check_actions.py`** — a gate over `src-tauri/src/**/*.rs` and `docs/**/*.md` that:

* holds a registry of every broadcast action this project may emit, each with its AOSP file and line
  and a `reaches_pipeline` flag;
* refuses `SHOW_TEST_MESSAGE` by name, explaining that it does not exist rather than reporting a
  generic unknown action;
* refuses any action whose `reaches_pipeline` is false — the protected broadcasts and the
  non-exported internal actions — even where a document is only quoting it in a command;
* requires any new action to be registered with provenance first, so "does this exist?" is answered
  before the command is written;
* has a `--self-test` that proves it catches the three real mistakes and accepts the one real action.

It found two real actions that were in the code but unregistered when first run, which is the check
working as intended rather than a defect in it.

Run it with:

```
python3 tools/check_actions.py --self-test
python3 tools/check_actions.py
```

---

## The commands that are worth implementing, and the one that decides everything

Ranked by what they can actually establish:

1. `adb shell getprop ro.debuggable` — **the** read. `1` means the telephony test receiver exists;
   anything else means no native root-free path.
2. `adb shell pm list packages | grep -i cellbroadcast` — which receiver package this A35 ships.
3. `adb shell dumpsys package <package> | grep -i "receiver\|exported\|SECRET_CODE"` — whether the
   OEM kept the receiver and the 2627 filter.
4. `adb shell am start -n <package>/.CellBroadcastListActivity` — open the history list to check
   for a delivered row *after* a genuine attempt.
5. Only if (1) is `1`: the `TEST_TRIGGER_CELL_BROADCAST` broadcast, with logcat as the sole verdict.

All five are read-only except the activity launch, which changes only the foreground screen. None
requires root, and none requires installing anything on the phone.

---

## Evidence index

| Claim | Label | Source |
|---|---|---|
| `cell_broadcast_test_alerts` does not exist | `CONFIRMED` | absent from both modules (14/15) and the framework manifest |
| `show_option_to_opt_out_notifications` does not exist | `CONFIRMED` | same |
| Alert preferences are per-SIM `SharedPreferences`, not global settings | `CONFIRMED` | `CellBroadcastSettings.java` lines 107–137, 245, 286–291 |
| `SHOW_TEST_MESSAGE` does not exist | `CONFIRMED` | absent from both modules on both branches |
| `SMS_CB_RECEIVED` is protected | `CONFIRMED` | `frameworks/base/core/res/AndroidManifest.xml:749` |
| `CellBroadcastListActivity` is exported and read-only | `CONFIRMED` | `packages/apps/CellBroadcastReceiver/AndroidManifest.xml` |
| The only injection action is `TEST_TRIGGER_CELL_BROADCAST`, gated on `ro.debuggable` | `CONFIRMED` | `GsmInboundSmsHandler.java` lines 53–110 |
| Samsung's package name and whether it kept these components | `UNKNOWN` | requires `A35-RO-001` tasks 3 and 15 |
