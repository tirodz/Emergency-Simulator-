# EXP-15: Windows controller end to end

**Status:** COMPLETE
**Date:** 2026-09-19
**Objective:** Determine whether a PC application can drive the already-proven Android Cell
Broadcast test mechanism reliably, and detect the outcome from evidence rather than assumption.

---

## Objective

Build the smallest usable PC controller for the proven injection path, and establish that it can:

1. discover attached devices and assess readiness;
2. verify root and refuse to send without it;
3. establish the test-alert prerequisites on the device;
4. require explicit operator confirmation;
5. inject through the existing Java injector;
6. determine the outcome from downstream Cell Broadcast evidence;
7. report CANCEL honestly, including what it cannot do.

---

## Hypothesis

A Python controller using `adb` can drive `android/alertinject` on a userdebug target and reliably
observe the genuine emergency-alert pipeline. Success can be distinguished from a silent no-op by
inspecting logcat for the production components, since an injector process can exit 0 while the
message is filtered.

---

## Setup

| Item | Value |
| --- | --- |
| Host | Debian 13, 4 vCPU, 15 GB RAM, no KVM |
| Target | AVD `test35`, `emulator-5554`, Android 15 / API 35 |
| Build | `userdebug`, `ro.debuggable=1` |
| Fingerprint | `google/sdk_gphone64_x86_64/emu64xa:15/AE3A.240806.043/12960925:userdebug/dev-keys` |
| CellBroadcast package | `com.google.android.cellbroadcastreceiver` |
| ADB | Android SDK Platform Tools |
| Controller | `tools/test_alert.py`, `app/controller.py` |

Cold boot without KVM takes roughly nine minutes, so the emulator was started before coding began.

---

## Commands

```bash
export ADB_PATH=/opt/android-sdk/platform-tools/adb

python3 tools/test_alert.py --list          # discovery and readiness
python3 tools/test_alert.py --dry-run       # validate, change nothing
python3 tools/test_alert.py --yes           # send, with confirmation skipped
python3 tools/test_controller.py            # safety invariants, no device needed
DISPLAY=:99 python3 tools/test_ui.py        # GUI widget tree under Xvfb
```

---

## Expected result

Discovery reports one READY device; the dry run passes every check and changes nothing; the send
reaches `CellBroadcastReceiver`, `CBAlertService`, `CellBroadcastAlertAudio` and
`CellBroadcastAlertDialog`; a history row appears; the controller reports `ALERT_DISPLAYED`.

---

## Actual result

**Discovery.** Correct readiness assessment:

```
SERIAL                STATE          ANDROID   ROOT   CELLBROADCAST
emulator-5554         READY          15        yes    com.google.android.cellbroadcastreceiver
```

**Dry run.** Changed nothing, proven by hashing the receiver's preference file before and after:

```
before: fce0e6bd951c3579d252e9ed96246ffc822e166072666b8650e998fc553ee074
after:  fce0e6bd951c3579d252e9ed96246ffc822e166072666b8650e998fc553ee074
```

**Send.** The full chain was observed:

```
  injector:   serviceCategory = 4355
  injector:   warningType     = 3 (ETWS TEST MESSAGE)
  injector:   body            = TEST ALERT - SIMULATION
  evidence: CellBroadcastReceiver.onReceive -- message accepted by the receiver
  evidence: CellBroadcastAlertService.onStartCommand -- alert service started
  evidence: CellBroadcastAlertAudio -- genuine alert audio engaged
  evidence: CellBroadcastAlertDialog -- genuine full-screen alert displayed
  Result: ALERT_DISPLAYED
```

**History database.** Genuine rows, including one that captured a bug (see below):

```
sqlite3 .../cell_broadcasts_v13.db 'select _id,service_category,serial_number,body from broadcasts;'
7|4355|1|TEST ALERT - SIMULATION
6|4355|1|TEST DRILL - HOUSEHOLD DEVICE
5|4355|1|TEST ALERT - SIMULATION
4|4355|1|TEST
```

**Negative cases.**

| Case | Result |
| --- | --- |
| No device attached | `NO_DEVICE`, exit 1 |
| Device not root (synthetic adb) | `NO_ROOT`, exit 1, send blocked |
| Hazard body | `REFUSED: must begin with 'TEST'`, exit 3 |
| Operator declines confirmation | `Cancelled. Nothing was sent.`, exit 4 |
| Wait for evidence with a signal | Stops and reports what was seen |

---

## Bugs found in testing

These are the reason the experiment was worth running rather than assuming.

**1. The alert body was silently truncated.** `adb shell` flattens its arguments into a single string
that the *device's* shell re-parses. `TEST ALERT - SIMULATION` arrived as `TEST`. The injector ran,
exited 0, and the alert appeared — with the wrong text, and no error anywhere. Captured in the
history database as row `4|4355|1|TEST`. Fixed by quoting every remote argument with `shlex.quote`.

**2. The secret code was being treated as a setter.** It is a toggle. Blindly sending it would
disable a working configuration. Fixed by reading state first and sending it only when testing mode
is off.

**3. A preference read failure looked like "disabled".** The emulator's shell is already uid 0 and
has no `su`, so a `su`-only read path failed. The dry run then wrongly reported it *would* enable
settings that were already on. Fixed by making file access root-mode aware (uid-0 shell first, then
`su`), and by making the controller refuse — rather than guess — when a read genuinely fails.

Bug 1 is the significant one: the failure mode was a plausible-looking success.

---

## Conclusion

The controller works end to end and the outcome detection is sound. A clean injector exit code is not
treated as success; the result comes from the production components in logcat. The dry run is verified
not to mutate device state. All safety refusals behave as specified.

The GUI drives the same controller through the same methods, and its safety behaviour was exercised
by building the real widget tree under Xvfb.

**Packaging.** `packaging/Emergency-Simulator.spec` builds a single windowed executable. A local
build produced a 12 MB onefile binary that ran against the live emulator, discovered the device and
wrote its log to the per-user path. The GitHub Actions workflow on `windows-latest` produced
`Emergency-Simulator.exe` (11.0 MB, DOS `MZ` header), uploaded as the artifact
`Emergency-Simulator-windows`.

---

## Next step

`EXP-ALERT-002` remains open: lock-screen full-screen presentation, vibration and DND override on a
genuinely locked screen. The device was never locked during these tests, so those behaviours are
unverified. See [open-questions.md](../open-questions.md).

After that: Wi-Fi transport, then multi-device fan-out.

---

## Safety

No cellular transmission occurred. No modem, radio or RF path was involved. No real hazard category
was reachable. Every alert was labelled `TEST`, and nothing was sent without explicit operator
confirmation.