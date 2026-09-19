# Progress

> This file is the live state of the project. A future session must be able to continue from here
> without reading the whole repository. Never leave it describing an outdated state.

## Current status

**Mission 2A — Android/AOSP test environment — CHECKPOINT 1 COMPLETE.**
An Android **userdebug** target is running and the production Cell Broadcast pipeline has been reached
from a root identity. The AOSP test application was proven **absent on a live device**, confirming the
earlier offline finding and ruling out the Mission 2B plan of driving it.

**Previous phase:** Phase 1 research / feasibility — COMPLETE (offline image analysis in Experiment 3).

The central question:

> Can a PC or controller phone legitimately cause an Android device to process a controlled Cell
> Broadcast test message through Android's *genuine* emergency-alert subsystem?

**Answer so far:** the production pipeline is reachable on a **userdebug** device via `adb root`, and
the protected-broadcast gate is passed by root — but the *message payload* is the real injection
point, and the AOSP test app that would normally supply it does not exist on any shipping image.

## Environment — established this mission

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

**Mission 2A — build the Android/AOSP test environment and establish the injection boundary.**

Checkpoint status:

| Checkpoint | Status |
| --- | --- |
| 1 — Environment assessment | **DONE** (`docs/environment.md`, `docs/environment-setup.md`) |
| 2 — AOSP source/image setup | **PARTIALLY DONE** — target running; source build BLOCKED and documented |
| 3 — Build/install | **BLOCKED** — no AOSP build capability here; test APK cannot be produced locally |
| 4 — Test app verification | **DONE, as a negative result** — test app confirmed absent |
| 5 — Manual alert test | **PARTIALLY DONE** — broadcast reaches receiver; payload delivery is the open problem |
| 6 — ADB automation | NOT STARTED |
| 7 — Minimal Windows controller | NOT STARTED |


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

Ordered by value. **Item 1 is the decisive open question.**

1. **EXP-ALERT-002 — deliver a real `SmsCbMessage` payload.** The broadcast gate is proven passable by
   root; the missing piece is a correctly formed `SmsCbMessage` under extra key `"message"`.
   Candidate approaches, cheapest first:
   a. Build the message in an **on-device** process that is permitted to construct the hidden type —
      e.g. a small helper APK using platform stubs, or better, checking whether the *receiver app
      itself* exposes any exported entry point we have not yet enumerated.
   b. Check whether `CellBroadcastReceiverTests` is obtainable prebuilt from any official source
      (CI artifacts, prebuilt GSI test suites) — **do not assume**; verify.
   c. Trace `SmsCbMessage`'s constructor and `Parcel` layout to determine whether a hand-crafted
      parcel is feasible in principle. This is a research step, and must be evaluated against the
      safety rules: it must produce a *test-labelled* alert through the genuine service.
2. **EXP-ALERT-003 — enable a displayable channel range.** Recall `range != null && range.mDisplay`
   gates the database write, and `channelManager.isEmergencyMessage()` decides the full-screen path.
   Determine from the live device which channels are enabled and which classify as emergency, so the
   test message lands on a channel that produces the genuine full-screen experience.
3. **Verify the alert experience** objectively: screen wake, full-screen dialog, sound, vibration,
   notification, database row, dismissal.
4. Only then, **Checkpoint 6**: ADB automation of whatever sequence is proven.

Do **not** build the Windows controller or the multi-device fan-out yet.


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
* Remote: `origin` = `tirodz/Emergency-Simulator-.git`.
* AppOps and SELinux have **not** been exercised yet.

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
| _(this commit)_ | chore: establish Android test environment and record the injection boundary |

**Mission 2A Checkpoint 1 is complete.** The next session should start the emulator immediately (nine
minute boot without KVM), then tackle **EXP-ALERT-002** — delivering a real `SmsCbMessage` payload
into the genuine pipeline. Building the AOSP test app is not possible in this environment and is no
longer on the critical path.

