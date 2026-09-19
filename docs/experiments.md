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

## Experiment 3 — Inspect a real AOSP system image (EXECUTED)

**Status:** CONFIRMED — executed offline against a Google-published AOSP image
**Risk:** none

### Why this variant was run

The execution environment for this run had no ADB, no Android SDK, no JDK and no KVM
(`CapEff: 0000000000000000`, no `/dev/kvm`, CPU exposes only `hypervisor`). That makes an
emulator impossible, so the device experiments (1, 2, 6, 7) could not be run. This
experiment was designed to extract the maximum verifiable evidence **without a device**, by
reading a real Google AOSP system image byte-for-byte.

### Objective

Determine whether an AOSP image ships the Cell Broadcast components, establish from real
bytes what the privilege model is, and determine whether the AOSP test application is
included.

### Setup

```
Image: https://dl.google.com/developers/android/cinnamonbun/images/gsi/aosp_x86_64-exp-CP41.260828.004.A8-16319058-9aa638ec.zip
Size:  1,243,476,645 bytes
SHA-256: 9aa638ec20577ac4d15610527d2da2e7e3fc8388ae7ca23c2de3cb4e3df535c1
```

Build properties read from `build.prop` inside the archive:

| Property | Value |
| --- | --- |
| `ro.build.fingerprint` | `Android/generic_system/generic:17/CP41.260828.004.A8/16319058:user/release-keys` |
| `ro.build.type` | **`user`** |
| `ro.build.tags` | `release-keys` |
| `ro.debuggable` | **`0`** |
| `ro.secure` | `1` |
| `ro.adb.secure` | `1` |
| `ro.build.version.release` | `17` |
| `ro.build.version.sdk` | `37` |

This is a **user** build — the strictest configuration. Anything found here is a lower bound
on what a userdebug or eng build permits.

### Tooling

This environment has no root, no loop devices, no `debugfs` and no `simg2img`. Minimal
read-only tooling was written to read the image anyway and is kept in the repository:

| File | Purpose |
| --- | --- |
| `tools/ext4ls.py` | Read-only ext4 walker: superblock, group descriptors, inode table, extent trees, directory entries. Lists directories and extracts files. |
| `tools/findapks.py` | Recursively walks an image looking for path matches. |
| `tools/axml.py` | Minimal Android binary-XML (AXML) string-pool and element extractor. |

### Commands

```bash
python3 tools/ext4ls.py system.img /system
python3 tools/ext4ls.py system.img /system/apex/com.android.cellbroadcast.capex
python3 tools/findapks.py system.img cellbroadcast
```

The `.capex` container is a ZIP whose `original_apex` member is a second ZIP containing an
ext4 `apex_payload.img`; both layers were opened directly.

### Actual result

**1. Where the components live.** The receiver and service are no longer in
`system/priv-app`. They ship inside an APEX:

```
/system/apex/com.android.cellbroadcast.capex            6,090,752 bytes
    └── original_apex
          └── apex_payload.img   (ext4, 24,494,080 bytes)
                └── priv-app/
                      CellBroadcastApp@CP41.260828.004.A8/CellBroadcastApp.apk   23,944,068 bytes
                      CellBroadcastServiceModule@CP41.260828.004.A8/
```

APEX manifest name `com.android.cellbroadcast`, version code `0x01B1E89F` = 28,427,167.
This confirms that Cell Broadcast is a Mainline/APEX module.

**2. The test application is NOT shipped.** A recursive walk of the entire `system.img` for
the substring `cellbroadcast` returned only:

```
/system/apex/com.android.cellbroadcast.capex
/system/priv-app/CellBroadcastLegacyApp/...        (a separate legacy shim)
```

A recursive walk of the APEX payload for `test` returned **no matches**. A walk of the whole
image for `tests` returned only `libcts_flags_tests_rust.dylib.so`. A walk for `sl4a`
returned **no matches**.

`CellBroadcastReceiverTests` is build-time only, exactly as its `Android.bp` implies.

**3. The privileged permission allowlist, read from the shipping image.**
`/system/apex/com.android.cellbroadcast.capex/etc/permissions/com.android.cellbroadcastreceiver.module.xml`:

```xml
<privapp-permissions package="com.android.cellbroadcastreceiver.module">
    <permission name="android.permission.BROADCAST_CLOSE_SYSTEM_DIALOGS"/>
    <permission name="android.permission.INTERACT_ACROSS_USERS"/>
    <permission name="android.permission.MANAGE_USERS"/>
    <permission name="android.permission.STATUS_BAR"/>
    <permission name="android.permission.MODIFY_PHONE_STATE"/>
    <permission name="android.permission.MODIFY_CELL_BROADCASTS"/>
    <permission name="android.permission.READ_PRIVILEGED_PHONE_STATE"/>
    <permission name="android.permission.RECEIVE_EMERGENCY_BROADCAST"/>
    <permission name="android.permission.START_ACTIVITIES_FROM_BACKGROUND"/>
</privapp-permissions>
```

This is the authoritative list of what the real receiver may do.
`RECEIVE_EMERGENCY_BROADCAST` is granted only through this allowlist.
`START_ACTIVITIES_FROM_BACKGROUND` is what lets the alert dialog appear over the current app.

**4. The emergency action is a protected broadcast.** Extracted from `framework-res.apk`'s
binary `AndroidManifest.xml`:

```
protected-broadcast    android.provider.Telephony.SMS_CB_RECEIVED
protected-broadcast    android.provider.action.SMS_EMERGENCY_CB_RECEIVED
protected-broadcast    com.android.cellbroadcastreceiver.GET_LATEST_CB_AREA_INFO
```

Empirical confirmation from a shipping image that the `BroadcastController` `isCallerSystem`
gate applies.

**5. The exact action string.**
`android.provider.action.SMS_EMERGENCY_CB_RECEIVED` — note `provider.action`, **not**
`provider.Telephony`. The non-emergency action is
`android.provider.Telephony.SMS_CB_RECEIVED`. A wrong action string is a silent no-op, so
this is recorded explicitly.

**6. Alert tones ship in the APK.**

```
res/raw/default_tone.ogg
res/raw/etws_default.ogg
res/raw/etws_earthquake.ogg
res/raw/etws_other_disaster.ogg
res/raw/etws_tsunami.ogg
res/raw-mcc302/...  (Japan)      res/raw-mcc334/...  (Mexico)
res/raw-mcc440/...  (Japan)
```

Confirms ETWS tones ship with the module and that tone selection is MCC-dependent.

**7. Shell wrappers present in the image** (relevant to what `adb shell` reaches):

```
/system/bin/pm        -> cmd package "$@"
/system/bin/am        -> cmd activity "$@"   (instrument handled separately)
/system/bin/appops    -> cmd appops "$@"
/system/bin/settings  -> cmd settings "$@"
```

### Conclusion

- The receiver and service ship as an APEX module and are present on a **user** build.
- The AOSP **test** application does **not** ship on any build type. It is build-time only.
- `SMS_EMERGENCY_CB_RECEIVED` and `SMS_CB_RECEIVED` are both `<protected-broadcast>` in the
  shipping framework.
- The receiver holds `RECEIVE_EMERGENCY_BROADCAST` only as a privileged permission granted
  via the allowlist above; it is not obtainable by an ordinary app.

### Next step

Experiment 6 still needs a device. The exact command sequence is in Experiment 6 below.
In that emulator session, `adb shell ls /apex | grep -i cellbroadcast` and
`adb shell getprop ro.build.type` reproduce the above findings on a live system.

---

## Experiment 3b — Static answers to the Experiment 6 sub-questions

The device-dependent parts of Experiment 6 could not be run. The parts decidable from source
and from the shipping image are answered so the hardware run only has to confirm interactive
behaviour.

| # | Question | Answer | Status |
| --- | --- | --- | --- |
| 1 | Can the test APK be built/installed on an emulator or dev device? | Target exists: `android_test` `CellBroadcastReceiverTests`, `certificate: "platform"`, `platform_apis: true`, `instrumentation_for: "CellBroadcastApp"`. It is **not** in any `PRODUCT_PACKAGES`, so it is not installed by default even on userdebug; it must be built and installed deliberately, and the build must be platform-signed. | CONFIRMED (build config); device install UNKNOWN |
| 2 | Is `SendTestBroadcastActivity` exported? | Yes — `android:exported="true"`, identical on android14-release (`346bb74`), android15-release (`62e355a`), android16-release (`b97c8a4`), `main` (`17f1a4a`). | CONFIRMED |
| 3 | Can `adb shell am start` launch it? | UNKNOWN — needs a device. Exported + `MAIN`/`LAUNCHER` is necessary but **not sufficient**; the platform-signed install with `sharedUserId=android.uid.phone` must also succeed. | UNKNOWN |
| 4 | Exact component name and required extras? | `com.android.cellbroadcastreceiver.tests/.SendTestBroadcastActivity`. **No required extras** — the activity reads none. | CONFIRMED |
| 5 | Does launching alone trigger a message? | **No.** `onCreate()` only calls `setContentView` and wires `OnClickListener`s. No `onNewIntent`, no `getIntent()`, no send call anywhere in the lifecycle. Nothing is sent until a button is clicked. | CONFIRMED |
| 6 | If ADB input is required? | Buttons are standard `Button` widgets with stable IDs (`button_etws_test_type`, `button_gsm_cmas_monthly_test`, `button_gsm_state_local_test_alert`, ...). Drivable with `adb shell input tap x y` or `uiautomator` automation. Coordinates are resolution-specific and must be read at run time from `uiautomator dump`. | CONFIRMED (mechanism) |
| 7 | Capture logcat | `adb logcat -c` before, `adb logcat -v threadtime` during. Filters: `CellBroadcastReceiver`, `CellBroadcastAlertService`, `CellBroadcastAlertAudio`, `SendTestBroadcastActivity`, `ActivityManager`, `BroadcastController`. | CONFIRMED (command) |
| 8 | Does the production receiver handle it? | Yes by construction. The test code calls `sendOrderedBroadcastAsUser` with `SMS_EMERGENCY_CB_RECEIVED` (or `SMS_CB_RECEIVED`), explicit `setPackage()` to the default receiver, `receiverPermission=RECEIVE_EMERGENCY_BROADCAST`, `appOp=OP_RECEIVE_EMERGECY_SMS`. The receiver's `onReceive` is the genuine production entry point. | CONFIRMED (code path) |
| 9 | Screen / sound / vibration? | UNKNOWN — needs a device. The path requests alarm-stream audio, vibration, and a full-screen dialog with `FLAG_SHOW_WHEN_LOCKED`. | UNKNOWN |
| 10 | How is the message classified? | By the injected identifier. The test app can send ETWS (`0x1100`–`0x1103`), CMAS (`0x1112`–`0x111E`) or generic GSM/UMTS messages. Classification happens in the production receiver via `SmsCbConstants`, not in the test app. | CONFIRMED (mechanism) |
| 11 | Emulator vs userdebug vs user? | UNKNOWN — needs devices. The permission model is build-type independent; what differs is whether the platform-signed test APK can be installed at all. | UNKNOWN |

### The critical security question

**Does `exported=true` mean an ordinary external application can perform the privileged
injection? No.**

The privilege is not conferred by the activity being exported. It is conferred by the
*identity of the process that executes the send call* — the test app's own process.

| Stage | Identity / control |
| --- | --- |
| Caller (ADB `am start`) | shell UID 2000. May start the activity because it is exported. This only causes the activity to **render**. |
| Activity process | `com.android.cellbroadcastreceiver.tests`, running as `android.uid.phone` (sharedUserId), UID 1001. |
| Send call | Executed **inside that process**, by that identity — not by shell. |
| Permission checked | `RECEIVE_EMERGENCY_BROADCAST` — held because the APK is platform-signed and privileged-allowlisted. |
| Broadcast gate | `BroadcastController` sees `callingUid` = PHONE_UID (1001), which is in the `isCallerSystem` switch, so the protected-broadcast check passes. |
| AppOp | `OP_RECEIVE_EMERGECY_SMS` / `OP_RECEIVE_SMS` must be `MODE_ALLOWED` for the sender. |
| Receiver resolution | Explicit `setPackage()` to `com.android.cellbroadcastreceiver[.module]`. |
| Receiver permission | `RECEIVE_EMERGENCY_BROADCAST`, held by the receiver as a privileged permission. |

So: **the privileged operation is performed by the test application's own privileged
identity, after ADB merely launched its exported Activity.** ADB cannot perform the
injection itself. The security boundary is not bypassed — it is *satisfied* by an
application that legitimately holds the identity. This is why the path is legitimate rather
than an exploit.

The corollary matters most for the Windows EXE: **ADB is not the injection mechanism.** ADB
is the transport that reaches the test app. If the test app is absent — which it is on every
shipping image, including the GSI analysed above — ADB has nothing to launch, and there is
no ADB-only path to a genuine alert.

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
