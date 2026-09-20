# Progress

> This file is the live state of the project. A future session must be able to continue from here
> without reading the whole repository. Never leave it describing an outdated state.

## Current status

**Mission 3 — the stock-device question — STARTED. The evidence batch is served and waiting on the
operator.**

The question this mission must answer, and which nobody has answered yet:

> Is there **any legitimate, non-root** path from a Windows PC over Wi-Fi/IP that makes a
> **completely stock Samsung Galaxy A35** process a test alert through its **genuine
> `CellBroadcastReceiver`**?

Mission 3A is the read-only inspection that decides it. Batch **A35-RO-001** — ten read-only probes —
is served from the bridge and the operator has been given a one-paste PowerShell block that runs them
against the A35 over adb and posts the raw output back. See
[`docs/connect-one-paste.md`](docs/connect-one-paste.md).

**No command has ever been run against the A35.** The batch is authored and served; the operator has
not yet posted evidence. `evidence/` is empty.

**This session's work before the operator runs anything:**

* **A live bridge token was committed to this public repository** (`224624f` and earlier), along with
  a session host. The host is dead and the token is worthless, but the pattern would have leaked
  every future token identically. Fixed: the committed template holds placeholders only and
  `tools/make-paste.py` renders the live values from the gitignored `bridge-token.txt` into a
  gitignored file. This is a correction to the previous session's claim that the paste was ready to
  hand over.
* `AGENTS.md` added, recording the mode distinction, the evidence rule, the hardware constraints and
  the credential rule.
* The bridge's own guarantees re-verified this session: `/health` answers on the public URL confirmed
  by an independent external fetch (`r.jina.ai`), a missing or wrong token is rejected with 401, an
  evidence name attempting `../` traversal is refused, and a posted body round-trips byte-for-byte.

### Immediate next steps

1. Operator pastes the connection block and runs batch `A35-RO-001`.
2. Read the raw evidence — the answers are in the `dumpsys package` output. Do not skim it.
3. Answer, with evidence: which package owns alerts (AOSP, Samsung, or both); whether the emergency
   permission is `signature|privileged` (this single fact decides whether an ordinary app can ever
   trigger the broadcast); whether any component is exported in a way an external caller could
   legitimately reach; whether a legitimate test component exists at all; whether the module is an
   APEX and at what version; and the real carrier (CSC, `gsm.operator.numeric`).
4. Write findings to `docs/stock-device/aosp-vs-samsung.md` and
   `docs/stock-device/security-boundary.md`, with every claim labelled.
5. If a legitimate non-privileged entry point exists, author a second batch — and get the operator's
   explicit approval for that specific command first, because it crosses from inspection into an
   attempt. If none exists, that is the answer: document the boundary and stop.

### Carried over from the previous session

**Mission 2B — release hardening — COMPLETE. CI is green and the artifact is verified on Windows.**

The controller from Mission 2 now ships as a single Windows executable that carries its own Android
Platform Tools and its own injector, and a build can no longer be produced that is unable to find
those resources: the release verifies itself before the artifact is uploaded. The desktop interface
has been rebuilt as an instrument with explicit visual language, a device/support verdict board, a
live send gate and a permanent safety statement that no result can overwrite.

CI run [`35449434829`](https://github.com/tirodz/Emergency-Simulator-/actions/runs/35449434829) is
green on `windows-latest`, and the uploaded artifact reports:

```
  frozen      : True
  injector    : FOUND  ...\android\alertinject\out\alertinject.jar  (3385 bytes)
  bundled adb : ...\platform-tools\adb.exe
  adb         : BUNDLED
  adb version : Android Debug Bridge version 1.0.41
RESULT: OK
```

`adb: BUNDLED` also proves the companion DLLs were staged, since `adb.exe` fails at load time without
them. Verified locally too: a onefile build launched from an empty directory with no `ADB_PATH`
resolved its bundled injector and adb, discovered the live emulator and logged to the per-user path.
And the real interface, driven against `emulator-5554`, produced a genuine alert
(`CellBroadcastReceiver` → `CBAlertService` → `CellBroadcastAlertAudio` → `CellBroadcastAlertDialog`),
moved the gate to `DELIVERED`, disabled SEND and left the safety strip untouched.

Everything is pushed on the branch `release/self-contained-windows-build`, in PR
[#1](https://github.com/tirodz/Emergency-Simulator-/pull/1) (draft, awaiting review). Both CI jobs —
the Windows build and the Xvfb interface tests — are green on the head commit (`35450205427`).

## Current milestone

Mission 2B is delivered and verified. The next milestones, in order:

1. **Merge PR #1.** It is a draft; the branch is ready and CI is green.
2. **`EXP-ALERT-002`** — lock-screen presentation, vibration and DND override. Still the oldest open
   item, and still blocked on the same two things as before (see Blockers).
3. **Wi-Fi transport** — the controller is transport-agnostic; add a network path alongside adb,
   with pairing and authentication, keeping the same evidence-based result detection.
4. **Multi-device fan-out** — iterate the existing per-device send; do not build a parallel pipeline.

## What has been established (Mission 2B)

* **The release is self-contained, on the real platform.** Confirmed by CI on `windows-latest` against
  the uploaded artifact, and locally against a onefile build run from outside the repository.
* **The build verifies itself.** `app/main.py --selftest` reports the search roots, the injector, the
  bundled adb and its version. It writes a file when there is no console, which is the case for a
  windowed Windows build, and CI reads that file instead of trusting an exit code.
* **The committed injector is a deliberate trade.** One 3 KB jar is committed so a fresh checkout can
  produce a working release without a JDK or the Android SDK.
* **The send gate holds against a live device.** After a real alert the device is `DELIVERED`, SEND is
  disabled, Acknowledge is offered, and a second send is refused before the device is touched.
* **The safety statement is permanent.** Outcomes announce themselves on a separate strip, asserted
  by test, so the one line that must always be true cannot be replaced by a transient one.
* **CI catches platform-specific defects.** The first two CI runs each failed on a genuine
  bug (BUG-010 and the adb probe assertions) that passed locally on Linux. The Windows job is doing
  its job.

## Findings (Mission 2B)

### Confirmed facts

* A PyInstaller **windowed** build has `sys.stdout is None` on Windows, and it is a **GUI-subsystem
  binary**: invoking it directly from PowerShell returns immediately without waiting, so neither a
  file it writes nor its exit code is available yet. `Start-Process -Wait -PassThru` is required.
  This is BUG-010, found by CI.
* `adb.exe` fails at **load time** if `AdbWinApi.dll` / `AdbWinUsbApi.dll` are absent, which is
  indistinguishable from "no adb" to a file check. Discovery executes `adb version`; the build script
  asserts the DLLs are staged beside the binary.
* The injector jar was gitignored, so a fresh clone could not build a working release. Now committed;
  only `classes/` and `dex` remain ignored. `android/alertinject/out/.gitignore`.
* A frozen build resolves resources through `sys._MEIPASS` and then the executable's directory.
  Verified on both Linux and Windows.
* **A test that asserts a platform-specific error string is not portable.** The adb probe test
  asserted a POSIX exit status of 127, which Windows cannot produce. The property under test is that a
  present-but-unusable executable is rejected; the reason is platform-specific, so only the POSIX
  branch may check it.
* `python3-tk` installs against the **distribution** Python, so a `setup-python` build may have no
  tkinter. The interface CI job uses `/usr/bin/python3`.

### Hypotheses (not yet verified)

* The Windows artifact has not been driven against a device on Windows: CI has no device. The
  device-driving path is identical Python and was exercised on Linux, but the Windows adb transport
  is untested end to end.
* Android 14 and 16 accept the same injection path. 15 is verified; 14/16 are inferred.
* The `TERM`/font fallbacks render as intended on a real Windows desktop with Segoe UI.

## Blockers

* **No KVM** on this host: the emulator uses software emulation and takes ~9 minutes to cold boot.
  Start it before doing anything else.
* **`EXP-ALERT-002` needs a locked device.** The device was never locked, so lock-screen full-screen
  presentation, vibration and DND override remain unverified.
* **Vibration specifically** was logged as `no pulsation pattern` on the emulator, so the emulator
  may not be able to demonstrate it at all.

## Next exact actions

1. **Merge PR #1** once reviewed. It is a draft; CI is green.
2. **`EXP-ALERT-002`** — lock the device (`adb shell input keyevent KEYCODE_SLEEP`), send an alert,
   and capture whether the dialog appears over the lock screen. Attempt vibration and DND override.
   Record in `docs/experiments.md`. If the emulator cannot show vibration, say so and mark it UNKNOWN
   rather than claiming success.
3. **Wi-Fi transport** — add a network path alongside adb, with pairing and a per-device token.

## Last known working state

* **Repo:** `/workspace/project/Emergency-Simulator-`, branch `main`, HEAD `11acbbd`.
* **Active branch:** `release/self-contained-windows-build`, pushed, PR #1 (draft), CI green.
* **Target:** AVD `test35`, `emulator-5554`, Android 15 / API 35, `userdebug`, `ro.debuggable=1`.
* **Run the CLI:** `ADB_PATH=/opt/android-sdk/platform-tools/adb python3 tools/test_alert.py --list`
* **Run the GUI:** `DISPLAY=:99 python3 app/main.py` (headless host) or `dist\Emergency-Simulator.exe`
  on Windows.
* **Self-test:** `python3 app/main.py --selftest` (or `--selftest=<path>` with no console).
* **Tests:** `python3 tools/test_controller.py` (all pass, no device needed),
  `DISPLAY=:99 python3 tools/test_ui.py` (all pass).
* **Build the exe locally:** `powershell -ExecutionPolicy Bypass -File packaging\build_windows.ps1`
  (stages Platform Tools, runs the tests, builds, then verifies the artifact).
* **CI artifact:** `Emergency-Simulator-windows`, from a green run.
* Boot the emulator first: `emulator -avd test35 -no-window -no-audio -accel off` (~9 min).

## Git commits

Mission 2B (most recent first):

| Commit | Subject |
| --- | --- |
| `9269862` | docs: record BUG-009, the safety statement being overwritten by a result |
| `699f50e` | docs: record Mission 2B — the self-contained, self-verifying release |
| `11acbbd` | feat: make the Windows release genuinely self-contained and verifiable |
| `1fed923` | feat: rebuild the desktop interface as a laboratory instrument |
| `ea7b5a4` | docs: add a bug journal and tests for the hard-won invariants |
| `7c4e857` | feat: make the runtime self-contained and locate resources reliably |

Plus, on the release branch and not yet on `main`: the adb-probe portability fix and the
`Start-Process -Wait` fix, with BUG-010 recorded.

Mission 2:

| Commit | Subject |
| --- | --- |
| `ec40f49` | feat: add test alert controller core |
| `420ef33` | feat: add Windows desktop interface |
| `6851814` | feat: add Windows packaging and a CI build |

## Archive — Mission 1 and 2A (complete)

**Mission 2A — Android/AOSP test environment — CHECKPOINT 3 COMPLETE. THE CHAIN IS PROVEN.**

The central question is answered affirmatively, with raw device evidence:

> Can a PC or controller phone legitimately cause an Android device to process a controlled Cell
> Broadcast test message through Android's *genuine* emergency-alert subsystem?

**Answer: YES.** A locally built tool running as root has injected a constructed `SmsCbMessage` into
the stock `CellBroadcastReceiver` on a live Android 15 target, and Android's own components produced
the real alert dialog (with our text rendered verbatim), real alert audio, real text-to-speech of the
message body, a screen-hold, and a row in the real history database. No AOSP build, no platform
signature and no system-app install was needed.

The previous plan — build and drive AOSP's `CellBroadcastReceiverTests` — was **abandoned as
unnecessary**. `CellBroadcastReceiver` accepts its input as a `SmsCbMessage` in a broadcast extra, so
any tool that can construct one and pass the protected-broadcast gate is equivalent. See
[`docs/experiments.md`](docs/experiments.md) EXP-ALERT-002 and the correction in
[`docs/aosp-test-path.md`](docs/aosp-test-path.md) §10.

**Previous phases:** Phase 1 research / feasibility — COMPLETE. Mission 2A checkpoints 1 and 2 —
COMPLETE.

### Environment — established in Mission 2A

| Capability | Verdict |
| --- | --- |
| Android emulator | **AVAILABLE** — boots under software emulation (QEMU TCG) with `-accel off` |
| KVM / hardware virtualization | **BLOCKED** — no `vmx`/`svm`, `/dev/kvm` impossible to create even as root |
| Full AOSP source build | **BLOCKED** — needs 100–250 GB source + 50–150 GB out; we have 59+25 GB, 15 GB RAM, no swap |
| adb / platform-tools | AVAILABLE — `37.0.1` |
| JDK | AVAILABLE — OpenJDK **21** (Debian 13 ships 21, not 17) |
| Android SDK | AVAILABLE — `/opt/android-sdk`, cmdline-tools 12.0 |

Target: `emulator-5554`, AVD `test35`, image `android-35;google_apis;x86_64`,
fingerprint `google/sdk_gphone64_x86_64/emu64xa:15/...:userdebug/dev-keys`, `ro.debuggable=1`, ~9 min
cold boot without KVM.

Details: [`docs/environment.md`](docs/environment.md),
[`docs/environment-setup.md`](docs/environment-setup.md).

## What has been established (cumulative)

* The genuine pipeline and its exact component boundaries (source-verified).
* The AOSP test application's identity, permissions, UID, signing and exact call path.
* Where the test path enters the real pipeline: at the broadcast/service boundary between
  `CellBroadcastService` (CBS) and `CellBroadcastReceiver` (CBR).
* The four security gates that block a normal app, and which one is load-bearing.
* The channel-configuration filters (`testing_mode`, `debug_build`, `display`) and user-toggle defaults.
* The ETWS-vs-CMAS test mechanisms and which categories are safe to use.
* No "rocket attack" alert class exists in Android; the visible body text is free-form.
* The genuine alert UI, sound and vibration depend on nothing from the modem.
* The Cell Broadcast module ships as an **APEX**, present even on a `user` build (Experiment 3).
* Both CB entry actions are `<protected-broadcast>` in the shipping framework (Experiment 3).
* The exact emergency action is `android.provider.action.SMS_EMERGENCY_CB_RECEIVED`.

### New this mission (live device, Mission 2A)

* **The emulator boots without KVM.** Software emulation is a workable fallback (EXP-ENV-001).
* **The production receiver/service are present and are the *Google* build of the APEX module**, not
  pure AOSP, plus a `/system/priv-app` legacy shim (EXP-ENV-002).
* **The AOSP test app is absent on a live device**, not merely by image analysis (EXP-ENV-002).
* **Shell (uid 2000) is refused the protected broadcast** with a `SecurityException` from
  `ActivityManagerService.broadcastIntentLockedTraced` (EXP-ALERT-001).
* **Root (uid 0) passes the gate**: the broadcast is delivered and
  `CellBroadcastReceiver.onReceive` runs (EXP-ALERT-001).
* **But the broadcast alone is inert**: `CBAlertService` bails with
  `received SMS_CB_RECEIVED_ACTION with no extras!`. No alert UI, sound or vibration occurred.
* **The authoritative data contract** is an `SmsCbMessage` Parcelable under the extra key
  `"message"`; the real delivery path is binder (`ICellBroadcastService`), not an Intent (EXP-ENV-003).

## Current milestone

**Mission 2A — build the Android/AOSP test environment and prove the genuine alert chain.**

Checkpoint status:

| Checkpoint | Status |
| --- | --- |
| 1 — Environment assessment | **DONE** (`docs/environment.md`, `docs/environment-setup.md`) |
| 2 — AOSP source/image setup | **PARTIALLY DONE** — target running; source build BLOCKED and documented |
| 3 — Build/install the test APK | **NOT NEEDED** — superseded; the test APK is not the mechanism |
| 4 — Test app verification | **DONE, as a negative result** — test app confirmed absent on the live device too |
| 5 — Manual alert test | **DONE** — genuine alert UI, sound and TTS produced (EXP-ALERT-002) |
| 6 — Establish the privilege boundary | **DONE** — root passes, shell is refused (EXP-ALERT-001) |
| 7 — Minimal Windows controller | NOT STARTED — this is the next deliverable |


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
* [x] Wrote minimal read-only tooling to analyse an image offline: `tools/ext4ls.py`,
      `tools/findapks.py`, `tools/axml.py`.
* [x] Downloaded a real Google AOSP GSI and executed Experiment 3 against it (image-level analysis).

### Mission 2A — completed this run

* [x] Installed JDK 21, the Android SDK, the emulator, and an `android-35;google_apis;x86_64` system
      image; created and booted the AVD `test35` under software emulation (EXP-ENV-001).
* [x] Confirmed no KVM is available and that this is not fatal: the emulator runs under QEMU TCG.
* [x] Recorded the Cell Broadcast component layout on the live target (EXP-ENV-002).
* [x] Established the authoritative delivery contract for a Cell Broadcast message (EXP-ENV-003).
* [x] **Wrote `android/alertinject/`** — a reflective `SmsCbMessage` builder and injector that runs
      under `app_process`, with `build.sh`, `run.sh` and a README. Warning type is pinned to the ETWS
      test type; bodies must start with `TEST`.
* [x] **EXP-ALERT-002: produced the genuine alert.** Real `CellBroadcastAlertDialog` with our text,
      real alert audio, real TTS of the body, screen held awake, and a row in the real history
      database.
* [x] **EXP-ALERT-003: found and documented the preference gate.** `enable_test_alerts` defaults to
      false and `isTestAlertsToggleVisible` requires testing mode; both must be set.
* [x] **Found the vendor-supported testing-mode trigger**: `am broadcast -a
      android.telephony.action.SECRET_CODE -d "android_secret_code://2627"`. It is a toggle, and it
      alone is not sufficient.
* [x] **EXP-ALERT-004: settled the CANCEL question empirically.** BACK and `CLOSE_SYSTEM_DIALOGS` are
      both ignored; the dialog registers an `OnBackInvokedCallback` specifically to swallow BACK. Only
      the alert's own button dismisses, and the dialog queues messages.
* [x] **Closed EXP-ALERT-001**: shell UID is refused with `Permission Denial: not allowed to send
      broadcast ... uid=2000`, using the same tool that root used successfully.
* [x] Corrected `docs/aosp-test-path.md` (§10), `docs/feasibility.md` (§1, §2) and `report.md`
      (addendum + Executive Conclusion) to reflect the proven mechanism.
* [x] Wrote `docs/device-cellbroadcast.md` for the live-target component layout.

Prior deliverables (still current):

| File | Status |
| --- | --- |
| `README.md` | done |
| `progress.md` | done (this file) |
| `report.md` | done (+ milestone addendum) |
| `docs/sources.md` | done |
| `docs/architecture.md` | done |
| `docs/android-cellbroadcast.md` | done |
| `docs/aosp-test-path.md` | done (+ §9 corrections) |
| `docs/privilege-model.md` | done (+ image-verified note) |
| `docs/android-version-compatibility.md` | done |
| `docs/oem-compatibility.md` | done |
| `docs/transport-options.md` | done (+ corrected ADB section) |
| `docs/protocol-and-alert-types.md` | done |
| `docs/feasibility.md` | done |
| `docs/experiments.md` | done (+ Experiment 3 and 3b) |
| `docs/security-and-safety.md` | done |
| `docs/open-questions.md` | done |
| `docs/critical-questions.md` | done |
| `tools/ext4ls.py`, `tools/findapks.py`, `tools/axml.py` | added this run |


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

### Confirmed facts (backed by a real AOSP image — Experiment 3)

16. The Cell Broadcast receiver and service ship inside an APEX,
    `/system/apex/com.android.cellbroadcast.capex` (manifest name `com.android.cellbroadcast`), as
    `CellBroadcastApp@<build>/CellBroadcastApp.apk` and `CellBroadcastServiceModule@<build>/`. They are
    present on a **`user`** build.
17. **`CellBroadcastReceiverTests` is absent from the image and from every `PRODUCT_PACKAGES`.** A
    recursive walk of the whole image found only the APEX and a separate `CellBroadcastLegacyApp`
    shim. It is build-time only — it ships on no build type.
18. `SendTestBroadcastActivity` has no `onNewIntent` override and never calls `getIntent()`. It is a
    pure GUI, so **no Intent can trigger a send**.
19. `tests/testapp/src/` contains no JUnit test classes, so **`am instrument` is not a non-UI trigger
    either**.
20. Both CB entry actions are `<protected-broadcast>` in the shipping `framework-res.apk`:
    `android.provider.Telephony.SMS_CB_RECEIVED` and
    `android.provider.action.SMS_EMERGENCY_CB_RECEIVED`.
21. The exact emergency action string is `android.provider.action.SMS_EMERGENCY_CB_RECEIVED`
    (`provider.action`, **not** `provider.Telephony`). A wrong string is a silent no-op.
22. The receiver's privileged allowlist, read from the shipping APEX, grants exactly:
    `BROADCAST_CLOSE_SYSTEM_DIALOGS`, `INTERACT_ACROSS_USERS`, `MANAGE_USERS`, `STATUS_BAR`,
    `MODIFY_PHONE_STATE`, `MODIFY_CELL_BROADCASTS`, `READ_PRIVILEGED_PHONE_STATE`,
    `RECEIVE_EMERGENCY_BROADCAST`, `START_ACTIVITIES_FROM_BACKGROUND`.
23. ETWS alert tones ship with the module and are MCC-dependent (`res/raw/`, `res/raw-mcc302/`,
    `res/raw-mcc334/`, `res/raw-mcc440/`).

### Confirmed facts (backed by a live userdebug device — Mission 2A, this run)

24. **The functional injection point is the `"message"` extra**, carrying a `SmsCbMessage`
    Parcelable, on the protected broadcast `android.provider.action.SMS_EMERGENCY_CB_RECEIVED`.
    `CellBroadcastService` is not involved at all on this path.
25. **Root (uid 0) can construct and deliver that payload.** `CellBroadcastReceiver.onReceive` logs
    the action with `(has extras)`.
26. **Shell (uid 2000) cannot**, even with a perfectly formed payload:
    `Permission Denial: not allowed to send broadcast android.provider.action.SMS_EMERGENCY_CB_RECEIVED
    from pid=..., uid=2000`. The gate is checked before the extras are examined.
27. **The genuine alert experience occurs.** Confirmed on the device:
    `CellBroadcastAlertDialog` at `RESUMED` with our text rendered verbatim under Android's own
    `ETWS test message` title; `CellBroadcastAlertAudio` playing the alert tone; TTS speaking the body
    (`Speaking broadcast text: TEST ALERT - SIMULATION`); `FLAG_KEEP_SCREEN_ON` added and later
    removed; and the message written to `broadcasts` in `cell_broadcasts_v13.db`.
28. **Test alerts are disabled by default and are dropped by preference before the UI**, logged as
    `ignoring alert of type 4355 by user preference`. The gate is
    `emergencyAlertEnabled && isTestAlertsToggleVisible() && enable_test_alerts`.
29. **`testing_mode` is the supported lever**, and it has a vendor mechanism:
    `am broadcast -a android.telephony.action.SECRET_CODE -d "android_secret_code://2627"`. It is a
    **toggle**, not a setter. `enable_test_alerts` is a separate toggle and must also be on.
30. **The receiving app's preferences live in a private file**,
    `/data/user_de/0/<cb-package>/shared_prefs/<cb-package>_preferences.xml`, and must be re-read by
    force-stopping the app after an edit.
31. **ETWS test channel is `0x1103` = 4355**, from AOSP
    `res/values/config.xml` → `etws_test_alerts_range_strings`. Undefined channels are rejected
    (`received undefined channels`) and dropped.
32. **A displayed alert cannot be remotely dismissed.** BACK and
    `android.intent.action.CLOSE_SYSTEM_DIALOGS` are both ignored, and the dialog registers an
    `OnBackInvokedCallback` specifically to swallow BACK. Only the alert's own button dismisses.
33. **The alert dialog queues messages.** Two sends before acknowledgement produced `OK (1/2)`;
    dismissal advances the queue rather than replacing the current alert. Blind retries are unsafe.

### Hypotheses (plausible, not yet verified)

* Vibration will occur for the test alert once a channel with a configured vibration pattern is used.
  The run logged `no pulsation pattern`, so the test path's pattern source is unexercised.
  -> next experiment.
* The alert will present full-screen over the lock screen, since the dialog sets the relevant flags,
  but the device was unlocked during every run. -> next experiment.
* A DND override is achievable for a test alert by setting `override_dnd`. -> next experiment.
* The same mechanism works on Android 14 and 16. The API and the protected broadcast are stable
  across these releases, but only Android 15 has been exercised on a device. -> Experiments 10–14.
* A rooted retail device behaves like our userdebug emulator *for this path*, since the gate is the
  UID check and the remaining requirement is a writable preference file. -> Experiments 10–14.

## Blockers

* ~~No device or emulator~~ — **CLEARED.** The emulator runs under software emulation without KVM.
* ~~The platform signing key problem~~ — **CLEARED.** The test APK is not needed.
* ~~SELinux and AppOps are unexamined~~ — **CLEARED for this path.** Neither had to be changed; the
  UID check is the load-bearing gate, and root passes it.
* **`adb root` is unavailable on stock retail builds.** This is the remaining deployment constraint,
  not a technical blocker. The mechanism needs root; how root is obtained is a product decision. On
  Pixel and other unlockable devices, rooting is a documented procedure; on locked devices it is not
  available.
* **Vibration, lock-screen and DND behaviour are untested** — see Hypotheses. Low risk, but unproven.

## Next exact actions

**The investigation is complete. The next task is implementation.** Materials: a working
`android/alertinject/` and a proven command sequence in `docs/experiments.md`.

1. **Build `tools/test_alert.py`** — the smallest local controller, per the exact specification at the
   end of `report.md`. No GUI, no Wi-Fi, no multi-device. It must: list devices, verify root, establish
   the test-alert prerequisites (secret code first, preference-file edit as fallback), require an
   explicit confirmation before sending, support `--dry-run`, and report the outcome by reading
   logcat.
2. Run it end to end against `test35`; record the transcript as Experiment 15.
3. Then, in order: vibration (set a channel with a pattern, feel/listen), lock-screen presentation
   (lock the device, inject, observe), DND override.
4. Only after all of the above: Wi-Fi transport, multi-device fan-out, device discovery, and UI.

Do **not** attempt remote dismissal of a displayed alert. It has been shown not to work, and the
controller must not present it as a capability.


## Last known working state

* Repository: branch `main`. Working tree clean after committing.
* **A live Android target is available and was used for this milestone:**
  * AVD `test35`, device `emulator-5554`
  * image `system-images;android-35;google_apis;x86_64`
  * fingerprint `google/sdk_gphone64_x86_64/emu64xa:15/AE3A.240806.043/12960925:userdebug/dev-keys`
  * `ro.build.type=userdebug`, `ro.debuggable=1`, SDK 35
  * boot command and env vars in [`docs/environment-setup.md`](docs/environment-setup.md)
  * **~9 minute cold boot without KVM — start it early in any future session**
* Toolchain: OpenJDK 21.0.12.1, Android SDK at `/opt/android-sdk`, adb `37.0.1`, emulator `37.1.11.0`.
* Host: Debian 13, 4 vCPU, 15 GB RAM, **no swap**, **no KVM** (see `docs/environment.md`).
* Environment: `/usr/local/bin/python` 3.13.15, git 2.47.3.
* Git identity: `TIRO <68867160+tirodz@users.noreply.github.com>` (the repository's existing identity).
* Repo tools available: `tools/ext4ls.py`, `tools/findapks.py`, `tools/axml.py`.
* **Working injector**: `android/alertinject/` — build with `./build.sh`, run with `./run.sh`.
  Verified buildable from a clean tree. Output jar is gitignored; rebuild takes seconds.
* Remote: `origin` = `tirodz/Emergency-Simulator-.git`.
* AppOps and SELinux were **not needed** on the proven path — neither was consulted.
* Proven end-to-end recipe: [`docs/environment-setup.md`](docs/environment-setup.md) §8.

## Git commit

* Base: `89ec245` — "Initial commit".
* Research phase, small logical commits on `main`:

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
| `e9ac703` | docs: record final commit list in progress.md |

* Milestone run, on `main`:

| Commit | Subject |
| --- | --- |
| `11050a6` | research: execute Experiment 3 against a real AOSP system image |
| `27cbeaa` | docs: record milestone outcome and correct the feasibility matrix |
| `1b08597` | docs: record final commit list in progress.md |
| `7ad0e71` | chore: establish Android test environment and record the injection boundary |
| `740156a` | feat: prove the genuine Cell Broadcast alert chain on a live device |
| `4bb720e` | docs: record the proven state in the README and progress logs |

**Mission 2 is complete and pushed.** The next session should start by booting the emulator (nine
minutes without KVM), then run **`EXP-ALERT-002`** (lock-screen, vibration, DND override). The
controller work is done; the remaining items are the deferred features listed in
`# Next exact actions`.



## Mission 3 - handover of the built executable

The operator asked for the Windows build so they can exercise it against a real device.

**The artifact.** CI run `35458378681` built `dist/Emergency-Simulator.exe` from commit `6a8d03a`,
this branch's head at the time of writing. It was downloaded from the run's
`Emergency-Simulator-windows` artifact and is served by the bridge at `/download/`.

| Property | Value |
| --- | --- |
| Size | 19,636,828 bytes |
| SHA-256 | `52c0767d66692a196d263b151b3495193ef4f3c7295cee01e2f09c2279ee5d04` |
| Type | PE32+ x86-64, GUI subsystem, PyInstaller onefile |
| Bundles | `platform-tools\adb.exe`, `android\alertinject\out\alertinject.jar` (3,385 bytes) |

**Verified CONFIRMED:**

* The binary is a genuine Windows x86-64 GUI executable; the PE header was inspected directly.
* CI ran the artifact's own `--selftest` from a directory that is not the repository, and it
  reported `frozen: True`, `injector: FOUND`, `adb: BUNDLED`, `RESULT: OK`. That is the closest
  thing to a run on Windows available here, and it ran on the platform's own Windows runner.
* The bridge serves it byte-for-byte: the downloaded copy and the served copy share a SHA-256, and
  path-traversal attempts against `/download/` all return 404.

**Not established:**

* **The download has not been proved to survive the public ingress at full size.** A request to the
  public hostname did return all 19,636,828 bytes with a matching hash, but it completed in 0.09 s,
  which is loopback speed and consistent with the request hairpinning back inside the container
  rather than traversing the proxy. `r.jina.ai` confirmed the public route reaches the bridge (it
  returned the bridge's 401), but it cannot fetch a binary. So it is **UNVERIFIED** that a laptop
  pulling the file over the internet receives it intact. The operator pulling it is the test.
* **The executable has never been run against a real Android device.** CI has no device. The bundled
  adb and injector resolve, but nothing has exercised the adb transport end to end.

**Next exact actions:**

1. Operator downloads `Emergency-Simulator.exe` and checks its SHA-256 against the value above.
2. Operator runs it and reports what the window shows. Launching it with `--selftest <path>` and
   sending back the report gives a real-machine confirmation of the frozen-resource paths.
3. Only then does "Windows EXE exercised against a real device" in the release gate have any
   evidence behind it.


## Mission 3D — polished desktop release packaging (2026-09-19)

**Status: IMPLEMENTED in the repository; release CI will provide the final Windows installer artifact.**

The desktop interface has been refreshed into a dark, layered control console inspired by the CMF Ringtone Tool reference: graphite surfaces, orange accent, compact navigation, status pills, metric cards, a large test-alert composer and a dedicated activity panel. The underlying controller and its safety gates remain the only path to Android operations.

The Windows workflow now validates the application/PE version match, builds the self-contained executable, builds an x64 per-user Inno Setup installer, hashes both artifacts and publishes them on version tags. The installer does not request administrator rights.

Version 1.1.0 is the coordinated desktop release version. Actual alert delivery remains restricted to the proven controlled-development path; stock retail devices are not claimed supported.


## Mission 3E — 1.1.0 Windows release candidate (2026-09-19)

**Release candidate status: CI GREEN for the full executable, GUI-only build, controller tests and desktop interface tests. Installer creation and upload were also confirmed on run 35464661558.**

The polished desktop console is now the main UI: dark layered panels, orange accent, sidebar navigation, status pills, metrics, target/device cards, test-alert composer and activity log. Its visual language is inspired by the separate CMF Ringtone Tool reference; no CMF project files or code are shared.

The release pipeline produces a self-contained x64 Emergency-Simulator.exe, a GUI-only review build, and a per-user Emergency-Simulator-Setup-1.1.0.exe. The installer requires no administrator rights.

The verified full executable from the successful Windows build was 15,776,936 bytes with SHA-256 3BA5F164D4E74286BCBFBF690CDAAED56D3172345454AC448E2D2A9EEB872501. The verified installer was 17,512,914 bytes with SHA-256 BCA8A498CFC4A1603F32862CBD1DC939E746DEAC948BB79695BD8925E5DCD58E.

The controller duplicate-send safety gate persists across restarts; an outstanding or uncertain alert cannot be forgotten by reopening the application.

Actual alert injection remains limited to the proven controlled-development path on a rooted/userdebug Android target. Stock retail Android without root is not claimed supported. No cellular transmission or RF path exists in the application.


## Mission 4 — Tauri + Liquid Glass desktop rebuild (2026-09-19)

**Status: REBUILD IN PROGRESS on branch rebuild/tauri-liquid-glass.**

The 1.1.0 Tkinter/Python desktop shell is being retired from the product path after visual and runtime defects were observed on a real Windows screenshot. The replacement desktop application uses Tauri 2 + Rust + HTML/CSS/JavaScript and is designed around the glassy visual language of the separate CMF Ringtone Tool frontend.

### Confirmed design requirements

* Translucent/frosted layered surfaces with backdrop blur.
* Dark ambient gradient background with soft green illumination.
* Emerald green glowing status dots and primary action.
* Crisp vector SVG icons; no raster UI icons.
* Custom undecorated Windows title bar with native Rust window controls.
* Midnight, OLED and Light themes.
* Emerald, Cyan and Orange accent choices.
* Capability-aware device states with explicit USB authorization help.
* Inline failure/toast handling so a controller error does not close the application.
* The safe ETWS TEST channel remains fixed at 4355 and message bodies must begin with TEST.

### Architecture

* Product UI: Tauri 2 WebView2 frontend in src/index.html.
* Desktop/controller backend: Rust in src-tauri/src/lib.rs.
* Windows packaging: Tauri NSIS.
* ADB and the controlled development-only injector JAR are bundled as resources.
* The Java injector remains only because SmsCbMessage is a hidden Android framework type; it is not the desktop application language.
* Python desktop modules are no longer part of the Windows runtime.

### Safety boundary

CONTROLLED DEVELOPMENT MODE remains the only demonstrated end-to-end alert path: rooted/userdebug Android target. STOCK DEVICE MODE remains unproven and is never shown as READY.

Success is still determined by downstream Android CellBroadcast evidence. ADB process exit status alone is not success.

### Verification plan

1. GitHub Actions builds the real Tauri application and NSIS installer on windows-latest.
2. The workflow validates frontend glass/interaction markers and bundles only the intended runtime resources.
3. A successful Windows build is installed and inspected by the operator before a release is published.
4. After the rebuild passes CI and device testing, main can receive a [release] commit for v2.0.0.


## Mission 4 completion note

Status: COMPLETE and merged to main on 2026-09-19.

The Windows product is now a Tauri 2 + Rust + HTML/CSS/JavaScript desktop application with the custom glass visual system. GitHub Actions successfully builds the Windows application and NSIS installer.

Galaxy A35 stock handling is diagnostic-only. Production user builds do not receive an adb root restart request. The app exposes device details, the local Samsung PNG hardware preview, a non-invasive dry run, and a direct Android Developer Options action. The previously demonstrated genuine CellBroadcast path remains restricted to controlled root/userdebug Android targets.

The previous v1.1.0 GitHub release is no longer present.

## Final v1.0.0 finish

The production branch now uses a dedicated Devices workspace rather than scrolling the Overview when Devices is clicked. The workspace is populated from the Rust/ADB backend and includes a local Samsung PNG hardware reference plus live read-only device metrics. The Windows build generates its icon set from the simple SVG source using Tauri's supported icon pipeline. The release workflow removes the obsolete v2.0.0 release/tag before publishing v1.0.0.
