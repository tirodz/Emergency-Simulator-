# Progress

> This file is the live state of the project. A future session must be able to continue from here
> without reading the whole repository. Never leave it describing an outdated state.

## Current status

**Phase 1 (research / feasibility) — COMPLETE for the source-analysis portion.**
**Milestone "Prove the ADB → AOSP test app → genuine alert path" — BLOCKED in this environment;**
**one compensating experiment (Experiment 3) executed against a real AOSP image.**

No implementation has begun, by design. The deliverables so far are the documentation set under
`docs/`, plus `report.md`, which together answer the central feasibility question with cited evidence.

The central question:

> Can a PC or controller phone legitimately cause an Android device to process a controlled Cell
> Broadcast test message through Android's *genuine* emergency-alert subsystem?

**Answer: yes — via the mechanism AOSP itself provides, on a device we control at the system level.
It is not possible on a stock, unrooted retail device, and not possible over ADB alone.**

**Refinement established this run (Experiment 3): the AOSP test application ships on *no* build type.
It is absent from a real Google AOSP image and from every `PRODUCT_PACKAGES`. So even on a
userdebug/eng device, the test APK must be built and installed deliberately. And because its activity
has no Intent-driven trigger, ADB cannot complete the chain alone — it must drive the UI.**

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
* **(new) The Cell Broadcast module ships as an APEX**, present even on a `user` build.
* **(new) The test APK ships nowhere**, and its activity is not Intent-drivable.
* **(new) Both CB entry actions are `<protected-broadcast>` in the shipping framework**, confirmed
  from the image rather than inferred.
* **(new) The exact emergency action string** is
  `android.provider.action.SMS_EMERGENCY_CB_RECEIVED`.


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
* [x] Confirmed the milestone's device-lab portion is impossible in this environment: no `adb`, no
      Android SDK, no JDK, `CapEff: 0000000000000000`, no `/dev/kvm`, CPU exposes only `hypervisor`.
      No emulator can start; no device can attach.
* [x] Wrote minimal read-only tooling to analyse an image offline: `tools/ext4ls.py`,
      `tools/findapks.py`, `tools/axml.py`.
* [x] Downloaded a real Google AOSP GSI (Android 17, SDK 37, `user` build,
      SHA-256 `9aa638ec20577ac4d15610527d2da2e7e3fc8388ae7ca23c2de3cb4e3df535c1`).
* [x] **Experiment 3 executed.** Extracted `system.img`, walked it, extracted the Cell Broadcast
      APEX, its payload, the production APK, its permission allowlist, and the framework's
      `<protected-broadcast>` declarations. Full results in `docs/experiments.md`.
* [x] Answered the non-device-dependent parts of Experiment 6 as Experiment 3b.
* [x] Updated `docs/aosp-test-path.md` (§9), `docs/privilege-model.md`, `docs/transport-options.md`,
      `report.md` (addendum + corrected feasibility matrix) with the new findings.

## Current milestone

Milestone: **"Prove the ADB → AOSP test app → genuine alert path"**.

**Status: BLOCKED on hardware. Not abandoned — the analysis stage is complete and the remaining work
is a device run.**

Outcome of the milestone so far, stage by stage:

| Stage | Verdict | Basis |
| --- | --- | --- |
| ADB reaches the device | YES | normal transport |
| Exported test activity exists on a shipping device | **NO** | absent from the image and from `PRODUCT_PACKAGES` |
| ADB can launch the activity (if installed) | yes | `exported="true"` on 14/15/16/main |
| Launching alone sends a message | **NO** | no `onNewIntent`, no `getIntent()`; pure GUI |
| A UI tap invokes `SendTestMessages` | YES | standard `OnClickListener`s, stable button IDs |
| Injection uses the real action | YES | `android.provider.action.SMS_EMERGENCY_CB_RECEIVED` |
| Send passes the security gates | YES, *as the test app* | platform-signed, `android.uid.phone` (UID 1001) |
| Production receiver handles it | YES | explicit `setPackage()` + `receiverPermission` |
| Genuine UI / sound / vibration | **UNKNOWN** | needs a device |

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

### Hypotheses (plausible, not yet verified)

* ~~`am start` of the AOSP test app's activity from ADB produces a genuine alert.~~
  **REFUTED as an ADB-only path** by Experiment 3: the test APK ships nowhere, and launching the
  activity sends nothing without a UI tap. The remaining hypothesis is narrower: *after* the
  platform-signed test APK is installed on a userdebug/eng or rooted device, a UI tap driven over ADB
  produces a genuine alert. -> Experiment 6.
* A rooted device can satisfy the remaining gates (AppOps, SELinux) and inject the broadcast.
  -> Experiment 7.
* ~~An AOSP userdebug/eng emulator can host the full pipeline.~~
  **Partially answered**: the pipeline (APEX + receiver) is present even on a `user` GSI, so an
  emulator can host the *receiver*. Whether it can host the *test app* still requires building it.
  This environment cannot run an emulator (no KVM).
* An OEM that ships the AOSP receiver behaves identically to AOSP for the injection path. ->
  Experiments 10–14.

## Blockers

* **No physical Android device or emulator is available in this environment.** This is the hard
  blocker for the current milestone. There is no further offline analysis that can advance the
  decisive question. Required: a device with `adb` access that is either an AOSP userdebug/eng build
  or rooted.
* **The platform signing key problem.** Installing the AOSP test app requires the key the target build
  is signed with. A custom AOSP build provides it; a stock retail device does not. -> Experiment 4.
* **The test APK must be built, not fetched.** `m CellBroadcastReceiverTests` against a matching tree
  is unavoidable.
* **SELinux and AppOps are unexamined.** Both could still close the rooted path. -> Experiment 7.
* **The trigger is UI-driven.** `uiautomator`-based coordinate resolution is required; this is a
  fragility risk for the eventual controller, though not a blocker.

## Next exact actions

Small, ordered, actionable. **All require hardware.**

1. **Experiment 6 (highest value).** On a userdebug/eng or rooted device:
   build `CellBroadcastReceiverTests`, install it, then:
   * `am start -n com.android.cellbroadcastreceiver.tests/.SendTestBroadcastActivity`
   * confirm from `logcat` that **nothing** is sent without a tap (validates finding 18)
   * `uiautomator dump` to resolve button coordinates, then `input tap`
   * capture `logcat -v threadtime` and record whether a genuine alert appears.
2. **Experiment 4.** Build and install the test APK from a matching AOSP tree.
3. **Experiment 5.** Confirm the alert reaches the genuine UI/sound/vibration.
4. **Experiment 7.** Establish the minimal privilege set for a custom helper.
5. **Experiments 1 and 2** on the same device: read-only inspection of settings and toggles.
6. **Experiment 15** only after 1–5: the smallest ADB-driven controller.

Do **not** start the GUI, the Wi-Fi transport, or the multi-device fan-out before 1–5 conclude.


## Last known working state

* Repository: branch `main`. Working tree clean at the end of this run after committing.
* Environment: `/usr/local/bin/python` (Python 3.13), git, `curl`. **No** `adb`, **no** Android SDK,
  **no** JDK, **no** KVM (`CapEff: 0000000000000000`, no `/dev/kvm`). `unzip`/`apt-get` unavailable.
* Git identity: `TIRO <68867160+tirodz@users.noreply.github.com>` (the repository's existing identity).
* No Android builds have been attempted. No device or emulator has been used. No instrumentation tests
  have been run.
* Repo-level validation performed this run: internal Markdown link check (no broken links).
* Image analysis artefacts (not committed; large):
  * `/tmp/gsi/gsi_aosp_x86_64.zip` — 1,243,476,645 bytes,
    SHA-256 `9aa638ec20577ac4d15610527d2da2e7e3fc8388ae7ca23c2de3cb4e3df535c1`
  * `/tmp/gsi/system.img` — 2.30 GB ext4, 553,271 blocks
  * `/tmp/gsi/apex_payload.img`, `CellBroadcastApp.apk`, `framework-res.apk`, `framework.jar`
  * Only `tools/*.py` from this work is committed; the images can be re-downloaded from the URL in
    `docs/experiments.md`.
* AOSP reference clones live outside the repository (under `/tmp`). They are **not** required to
  continue: every path, branch and commit SHA is recorded in `docs/sources.md`.
* Remote: `origin` = `tirodz/Emergency-Simulator-.git`.

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

**Source-analysis phase and the offline portion of the milestone are complete. The next session must
be a hardware run: build and install `CellBroadcastReceiverTests` on a userdebug/eng or rooted device,
then drive its UI over ADB and observe whether a genuine alert results.**

