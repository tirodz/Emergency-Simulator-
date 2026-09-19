# Experiment plan

Every experiment has an objective, a hypothesis, a setup, commands, expected result, actual result,
and a conclusion. Results are filled in as experiments are executed. **No risky experiment is run
before the non-invasive ones.** Nothing in this plan transmits cellular signals.

Status legend: `NOT STARTED`, `IN PROGRESS`, `DONE`, `BLOCKED`.

---

## Experiment 1 — Inspect Android's existing Cell Broadcast settings and history

**Status:** NOT STARTED
**Risk:** none (read-only)

**Objective.** Establish the baseline state of the CB subsystem on a real device without changing
anything.

**Hypothesis.** A stock device exposes Cell Broadcast settings and an alert history; the receiver
package is present and enabled.

**Setup.** One Android device (any), USB debugging enabled.

**Commands.**

```bash
adb devices -l
adb shell getprop ro.build.version.release
adb shell getprop ro.build.version.sdk
adb shell getprop ro.debuggable
adb shell getprop ro.build.type
adb shell pm list packages | grep -i cellbroadcast
adb shell dumpsys package com.android.cellbroadcastreceiver | sed -n '1,120p'
adb shell cmd package query-receivers -a android.provider.action.SMS_EMERGENCY_CB_RECEIVED
adb shell cmd package query-receivers -a android.provider.Telephony.SMS_CB_RECEIVED
adb shell dumpsys activity broadcasts | grep -i cellbroadcast
```

**Expected result.** The receiver package is listed and privileged; both actions resolve to it; the
build type and `ro.debuggable` are recorded.

**Actual result.** _(pending)_

**Conclusion.** _(pending)_

**Next step.** If the receiver resolves correctly, proceed to Experiment 2.

---

## Experiment 2 — Is test-alert functionality exposed to the user?

**Status:** NOT STARTED
**Risk:** low (changing only user-visible settings)

**Objective.** Determine whether the "Test alerts" / "State and local tests" toggles exist and are
reachable on a stock device, and whether the `*#*#2627#*#*` testing-mode toggle works.

**Hypothesis.** On a stock AOSP-derived device the toggles are visible but off by default
(`test_alerts_enabled_default = false`, `state_local_test_alerts_enabled_default = false`); the secret
code may or may not be accepted depending on `ro.debuggable` and the
`allow_testing_mode_on_user_build` overlay.

**Setup.** Same device as Experiment 1. Open Settings → Safety & emergency → Wireless emergency alerts.

**Procedure.**

1. Photograph / record the available toggles and their default states.
2. Dial `*#*#2627#*#*` and observe whether a toast appears (`testing_mode_enabled`).
3. Re-open settings and see whether additional toggles appeared (exercise / operator-defined).
4. `adb shell run-as` is not applicable; instead read the app's prefs if the device is rooted:
   `adb shell cat /data/data/com.android.cellbroadcastreceiver/shared_prefs/*.xml` (root only).

**Expected result.** Toggles visible; defaults off; secret code behaviour determines whether testing
mode is reachable.

**Actual result.** _(pending)_

**Conclusion.** _(pending)_

**Next step.** Record which channels are enabled and which toggles are user-reachable. This feeds the
design of the "safe channel" choice.

---

## Experiment 3 — Can the AOSP emulator host the mechanism?

**Status:** NOT STARTED
**Risk:** none

**Objective.** Determine whether an AOSP/GSI emulator image contains the CellBroadcast apex and the CB
receiver, so the whole pipeline can be validated without hardware.

**Hypothesis.** **UNKNOWN.** A GSI ships the system image; whether the CBR app and the CB apex are
included is not established.

**Setup.** A userdebug AOSP emulator image or a GSI emulator.

**Commands.**

```bash
adb shell pm list packages | grep -i cellbroadcast
adb shell cmd package query-receivers -a android.provider.action.SMS_EMERGENCY_CB_RECEIVED
adb shell getprop ro.debuggable
adb shell ls /apex | grep -i cellbroadcast
```

**Expected result.** Ideally the apex and receiver are present. Absence is a legitimate negative
result and a blocker for emulator-based work.

**Actual result.** _(pending)_

**Conclusion.** _(pending)_

**Next step.** If present -> Experiment 4 on the emulator. If absent -> Experiment 4 needs hardware.

---

## Experiment 4 — Build and install the AOSP test application

**Status:** NOT STARTED
**Risk:** low, but requires a compatible build environment

**Objective.** Produce `CellBroadcastReceiverTests` from the same AOSP tree/branch as the device image
and install it.

**Hypothesis.** The module builds as an `android_test` with `certificate: "platform"` and can be
installed on a matching userdebug/eng device.

**Setup.** A checkout of the matching AOSP branch; `adb` targeting a userdebug/eng device whose
platform key matches the build.

**Commands (sketch).**

```bash
source build/envsetup.sh
lunch <target>-userdebug
m CellBroadcastReceiverTests
adb install -r $OUT/data/app/CellBroadcastReceiverTests/CellBroadcastReceiverTests.apk
adb shell pm list packages | grep cellbroadcastreceiver.tests
adb shell dumpsys package com.android.cellbroadcastreceiver.tests | grep -i sharedUser
```

**Expected result.** APK installs; package shares `android.uid.phone`; signing certificate matches the
platform.

**Actual result.** _(pending)_

**Conclusion.** _(pending)_

**Next step.** If the APK will not install, that is itself decisive evidence about the signing/UID
requirement. Record the exact error.

---

## Experiment 5 — Does the test path reach the genuine alert UI?

**Status:** NOT STARTED
**Risk:** low (produces a labelled test alert on a development device)

**Objective.** Confirm end-to-end that an injected test message produces Android's genuine alert.

**Hypothesis.** Yes: `CellBroadcastReceiver` → `CellBroadcastAlertService` → `CellBroadcastAlertAudio` +
`CellBroadcastAlertDialog`, with sound, vibration and a full-screen UI that appears over the lock
screen.

**Setup.** Experiment 4's device, with the "Test alerts" toggle enabled, testing mode enabled, device
locked with the screen off.

**Procedure.**

```bash
adb logcat -c
adb logcat -s CellBroadcastReceiver:* CellBroadcastAlertService:* CellBroadcastAlertDialog:* CellBroadcastAlertAudio:* &
# trigger one test message (activity UI or instrumentation)
```

**Expected result (acceptance criteria).**

1. `CellBroadcastAlertDialog` appears, full-screen, over the lock screen.
2. `CellBroadcastAlertAudio` plays `res/raw/*.ogg` on the alarm stream.
3. The device vibrates.
4. The message is inserted into the CB history provider.
5. logcat shows the genuine component names, not ours.

**Actual result.** _(pending)_

**Conclusion.** _(pending)_

**Next step.** This is the decision point for the whole project. See `feasibility.md` §8.

---

## Experiment 6 — Can ADB drive the test mechanism?

**Status:** NOT STARTED
**Risk:** low

**Objective.** Determine precisely how much ADB can do, and where it stops.

**Hypothesis.** `am broadcast` of the protected action from `shell` (UID 2000) throws
`SecurityException`; ADB cannot grant the needed identity.

**Commands.**

```bash
adb shell am broadcast -a android.provider.action.SMS_EMERGENCY_CB_RECEIVED \
    -n com.android.cellbroadcastreceiver/.CellBroadcastReceiver
adb shell am start -n com.android.cellbroadcastreceiver.tests/.SendTestBroadcastActivity
adb shell am instrument -w com.android.cellbroadcastreceiver.tests/androidx.test.runner.AndroidJUnitRunner
```

**Expected result.**

* `am broadcast` -> `SecurityException: Permission Denial: not allowed to send broadcast ...`
* `am start` of the test activity -> **may work**, because it launches a privileged app's own
  activity, which then does the privileged broadcast *as itself*. This is a plausible ADB-driven path
  and is worth testing carefully.
* `am instrument` -> depends on the test app being installed.

**Actual result.** _(pending)_

**Conclusion.** _(pending)_

**Next step.** If `am start` on the test activity produces a genuine alert, ADB-driven control is
viable and the controller design simplifies considerably. If not, a purpose-built privileged helper is
required.

---

## Experiment 7 — What privileges are actually necessary?

**Status:** NOT STARTED
**Risk:** medium (requires modifying a device's privilege configuration)

**Objective.** Determine empirically whether a helper running as `system`/`phone`, holding
`RECEIVE_EMERGENCY_BROADCAST`, is sufficient, and whether AppOps or SELinux block it.

**Hypothesis.** UID + permission are sufficient in principle; AppOps and SELinux are the two open
risks.

**Setup.** A rooted device. Install the helper as a priv-app, add it to a `privapp-permissions`
allowlist, give it `android:sharedUserId` compatible with a system UID (or run it as root).

**Procedure.**

1. Check the AppOp state: `adb shell appops get <pkg> RECEIVE_EMERGENCY_BROADCAST`.
2. Attempt the broadcast and capture the failure mode.
3. Check for SELinux denials: `adb shell dmesg | grep avc` and
   `adb shell cat /sys/fs/selinux/checkreqprot` (where available).
4. Iterate on the policy/permission until it works or a hard wall is reached.

**Expected result.** Either success (documented exactly) or a specific, named denial.

**Actual result.** _(pending)_

**Conclusion.** _(pending)_

**Next step.** Any denial must be recorded verbatim in this file, because it defines the project's
privilege requirement precisely.

---

## Experiment 8 — Can shell UID ever send the emergency broadcast?

**Status:** NOT STARTED
**Risk:** none

**Objective.** Resolve the remaining ambiguity in the protected-broadcast check.

**Hypothesis.** No, for the action itself. But the question of whether the `receiverPermission`
argument is also enforced for a *system* caller is unresolved and worth testing.

**Commands.**

```bash
adb shell am broadcast --user 0 -a android.provider.action.SMS_EMERGENCY_CB_RECEIVED \
    --receiver-permission android.permission.RECEIVE_EMERGENCY_BROADCAST \
    -n com.android.cellbroadcastreceiver/.CellBroadcastReceiver
adb shell service list | grep -i cellbroadcast
adb shell cmd -l | grep -i -E 'broadcast|cell'
```

**Expected result.** `SecurityException` from shell; no `cmd` service for CB injection exists in either
module (confirmed by source search — this is a structural fact, not a guess).

**Actual result.** _(pending)_

**Conclusion.** _(pending)_

---

## Experiment 9 — CANCEL semantics

**Status:** NOT STARTED
**Risk:** low

**Objective.** Determine exactly what can be cancelled and what cannot.

**Hypothesis.**
* Cancelling a *pending* command (before injection) is trivial and entirely under our control.
* Cancelling an *already displayed* system alert is not supported: `DISMISS_DIALOG` and
  `CellBroadcastAlertDialog.dismiss()` are internal to the privileged CBR app.

**Procedure.**

1. Inspect whether `CellBroadcastAlertService` exposes any exported dismissal entry point.
2. Attempt `adb shell am start` / `am broadcast` against any candidate action and record the refusal.
3. Confirm that the dialog's own dismiss button is the only supported removal path.

**Expected result.** Pending-cancel: yes. Remote-dismiss: no.

**Actual result.** _(pending)_

**Conclusion.** _(pending)_

**Next step.** Design the controller UI accordingly (cancel = cancel the pending send; dismiss must be
performed on the device).

---

## Experiment 10 — Stock Google Pixel baseline

**Status:** NOT STARTED
**Risk:** low (read-only unless a test build is flashed)

**Objective.** Characterise the Pixel as the reference OEM.

**Procedure.** Run Experiments 1, 2, 6, 7 on a stock Pixel; then (if permissible) flash a
userdebug/eng build and run 4, 5.

**Expected result.** Stock: receiver present, no injection possible. Flashable dev build: the test app
works.

**Actual result.** _(pending)_

---

## Experiment 11 — Device state interactions

**Status:** NOT STARTED
**Risk:** low

**Objective.** Determine how device state affects display of the injected alert.

**Hypothesis.** The alert wakes the screen and shows over the lock screen
(`FLAG_SHOW_WHEN_LOCKED`, `FLAG_TURN_SCREEN_ON`). No SIM should be fine because the path does not use
the radio, but subscription-driven channel configuration may matter. Airplane mode should be fine.

**Matrix to test (each with a test alert):** screen off / locked / unlocked / DND on / silent mode /
battery saver / no SIM / airplane mode / Bluetooth headset connected / headphones connected.

**Expected result.** Screen wake + lock-screen display confirmed. Behaviour under DND depends on the
channel's `override_dnd` and the global override setting. Audio routing to Bluetooth is an
**UNKNOWN**.

**Actual result.** _(pending)_

---

## Experiment 12 — Samsung baseline

**Status:** NOT STARTED
**Risk:** low

**Objective.** Determine how far Samsung diverges.

**Procedure.** Experiments 1, 2, 6 on a Samsung device. Check whether the AOSP receiver handles the
action or whether a Samsung component intercepts it.

**Expected result.** **UNKNOWN.** Record exactly what is found.

---

## Experiment 13 — Xiaomi / HyperOS baseline

**Status:** NOT STARTED
**Risk:** low

**Objective.** As above for Xiaomi.

**Expected result.** **UNKNOWN.**

---

## Experiment 14 — Motorola / Nothing baseline

**Status:** NOT STARTED
**Risk:** low

**Objective.** As above for the remaining OEMs. Lowest priority.

**Expected result.** **UNKNOWN.**

---

## Experiment 15 — Smallest possible local controller

**Status:** BLOCKED until Experiments 5 and 6 conclude

**Objective.** Build the smallest controller that can trigger a test alert on one device and cancel a
pending one.

**Design constraints.**
* One transport only (ADB).
* No GUI beyond a terminal prompt / single window with two buttons.
* Every send requires explicit confirmation.
* The payload text is always prefixed `TEST ALERT - SIMULATION`.
* No retries beyond a single idempotent attempt.

**Expected result.** A working PC → device → genuine alert path, with the alert labelled as a test.

**Actual result.** _(pending)_
