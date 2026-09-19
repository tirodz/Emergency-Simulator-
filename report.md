# Feasibility investigation report

**Subject:** Can a PC or controller phone cause Android test devices to experience a *genuine* Android
Cell Broadcast emergency alert through Android's own emergency-alert machinery?

**Phase:** 1 — source analysis, plus one executed experiment against a real AOSP system image
(Experiment 3). No device or emulator was available for this run: the execution environment has no
KVM, no ADB and no Android SDK. Every claim below is derived from AOSP source, from the framework's
own security code, or from bytes read out of a Google-published AOSP image — with the repository,
branch, commit and image hash recorded in [`docs/sources.md`](docs/sources.md) and
[`docs/experiments.md`](docs/experiments.md).

**Method:** clone and read the real components, trace the real execution path, quote the decisive
code, and where possible verify against a shipping image. Unknowns are marked `UNKNOWN` rather than
guessed.

---

## 1. Android Cell Broadcast architecture

Cell Broadcast is a point-to-multipoint messaging feature of GSM/UMTS/LTE/5G. A base station transmits
a short message to all devices on a cell; there is no per-subscriber addressing. Android receives the
raw PDU from the radio layer and hands it to a mainline module for decoding.

Android splits the work across **two mainline modules packaged in the `com.android.cellbroadcast`
apex**:

| Module | Repository | Package | Role | Identity |
| --- | --- | --- | --- | --- |
| CellBroadcastService (CBS) | `packages/modules/CellBroadcastService` | `com.android.cellbroadcastservice` | decode PDUs, build `SmsCbMessage`, emit broadcasts, own the history provider | `sharedUserId="android.uid.networkstack"` → runs as `NETWORK_STACK_UID`, in `android:process="com.android.networkstack.process"`. **This is why CBS passes the protected-broadcast check** |
| CellBroadcastReceiver (CBR), module variant | `packages/apps/CellBroadcastReceiver` | `com.android.cellbroadcastreceiver.module` | classify, filter, display, sound, vibrate, notify, persist | `privileged: true`, `certificate: "networkstack"`, no `sharedUserId` → its own UID |
| CellBroadcastReceiver (CBR), platform variant | same repo | `com.android.cellbroadcastreceiver` | same | `privileged: true`, `certificate: "platform"`, `system_ext_specific: true`, overrides `com.android.cellbroadcast`; no `sharedUserId` → its own UID |

A precise consequence: **CBS is a system UID (`NETWORK_STACK_UID`) and CBR is not** (neither variant
declares `sharedUserId`; verified in both manifests). CBS therefore passes the protected-broadcast test
by identity, while CBR — being the *receiver* — relies on its `privileged` status and privapp
allowlist to hold `RECEIVE_EMERGENCY_BROADCAST`.

The telephony framework's only role is forwarding: `frameworks/opt/telephony`,
`com.android.internal.telephony.CellBroadcastServiceManager` receives the raw modem callback and
forwards it over `ICellBroadcastService`.

## 2. Android emergency-alert architecture

The user-visible emergency experience is produced **entirely inside CBR**, by
`CellBroadcastAlertService` and the two components it launches:

| Effect | Component | Key mechanism |
| --- | --- | --- |
| Alert sound | `CellBroadcastAlertAudio` (service) | `AudioAttributes` `USAGE_ALARM` / `STREAM_ALARM`, alarm volume forced to max |
| Vibration | `CellBroadcastAlertAudio` | `Vibrator.vibrate(VibrationEffect, AudioAttributes)` |
| Full-screen UI | `CellBroadcastAlertDialog` (activity) | `FLAG_FULLSCREEN │ FLAG_SHOW_WHEN_LOCKED │ FLAG_TURN_SCREEN_ON │ FLAG_KEEP_SCREEN_ON` |
| Notification | `CellBroadcastAlertService` | emergency notification channel |
| History | `CellBroadcastContentProvider` | `Telephony.CellBroadcasts` |

Nothing in this layer reads the modem. That single fact is why the project is viable at all.

## 3. AOSP CellBroadcastReceiver

Verified structure and the exact reason it is unreachable from a normal app:

* `Android.bp` declares `privileged: true` and either `certificate: "networkstack"` or
  `certificate: "platform"`.
* `apex/permissions/com.android.cellbroadcastreceiver.module.xml` grants it
  `RECEIVE_EMERGENCY_BROADCAST`, `BROADCAST_CLOSE_SYSTEM_DIALOGS`, `STATUS_BAR`,
  `MODIFY_CELL_BROADCASTS`, `START_ACTIVITIES_FROM_BACKGROUND`, `MANAGE_USERS`, and more.
* `AndroidManifest.xml` declares `CellBroadcastReceiver` as `android:exported="true"` with an
  intent-filter containing the CB actions **and no `android:permission` attribute**.
* `onReceive` dispatches to `CellBroadcastAlertService` with **no caller check of its own**.

The security therefore rests entirely on the platform's broadcast enforcement, not on the app.

## 4. Android Telephony interaction

```
Modem/RIL
   │  unsolicited response (raw CB PDU)
   ▼
CellBroadcastServiceManager (frameworks/opt/telephony)
   │  ICellBroadcastService.handleGsmCellBroadcastSms(slotId, byte[])
   │  ICellBroadcastService.handleCdmaCellBroadcastSms(slotId, bearerData, serviceCategory)
   ▼
DefaultCellBroadcastService (CBS)
   │  GsmCellBroadcastHandler / CdmaCellBroadcastHandler decode the PDU
   ▼
SmsCbMessage
```

The AIDL (`telephony/java/android/telephony/ICellBroadcastService.aidl`) carries only `byte[]` and a
slot index. The framework never decodes a PDU; that is CBS's job.

## 5. `SmsCbMessage`

`frameworks/base`, `telephony/java/android/telephony/SmsCbMessage.java`. The class is `@hide` +
`@SystemApi`; all three constructors are annotated `@hide`. It is a `Parcelable`.

Fields relevant to this project:

| Getter | Significance |
| --- | --- |
| `getMessageBody()` | **free-form text** — the only field we can genuinely author |
| `getServiceCategory()` | the 16-bit channel/identifier; determines whether anything is displayed at all |
| `getSerialNumber()` | duplicate detection |
| `getMessagePriority()` | `isEmergencyMessage()` is `mPriority == MESSAGE_PRIORITY_EMERGENCY` |
| `getEtwsWarningInfo()` | non-null ⇒ `isEtwsMessage()` |
| `getCmasWarningInfo()` | CMAS class, severity, urgency, certainty |

## 6. Emergency broadcast intents

From `frameworks/base`, `core/java/android/provider/Telephony.java`:

| Action | Constant | Visibility | Required by receiver |
| --- | --- | --- | --- |
| `android.provider.Telephony.SMS_CB_RECEIVED` | `Sms.Intents.SMS_CB_RECEIVED_ACTION` | public SDK constant | `RECEIVE_SMS` |
| `android.provider.action.SMS_EMERGENCY_CB_RECEIVED` | `Sms.Intents.ACTION_SMS_EMERGENCY_CB_RECEIVED` | `@hide` + `@SystemApi` | `RECEIVE_EMERGENCY_BROADCAST` |

Both are declared `<protected-broadcast>` in `core/res/AndroidManifest.xml`.

## 7. Permissions

| Permission | Protection level | Role here |
| --- | --- | --- |
| `RECEIVE_EMERGENCY_BROADCAST` | `signature｜privileged` | the CMAS test broadcast's `receiverPermission` |
| `RECEIVE_SMS` | `dangerous` + `hardRestricted` | the generic/ETWS test broadcast's `receiverPermission` |
| `BROADCAST_SMS` | `signature` | declared by the test app |
| `MODIFY_CELL_BROADCASTS` | `signature｜privileged` | programming channels; **not needed** for the test path |
| `BROADCAST_CLOSE_SYSTEM_DIALOGS` | `signature｜privileged｜recents` | CBR closing the shade |
| `START_ACTIVITIES_FROM_BACKGROUND` | `signature｜privileged｜vendorPrivileged｜oem｜verifier｜role` | CBR's full-screen activity |
| `INTERACT_ACROSS_USERS_FULL` | `signature` | test app broadcasting across users |

## 8. AppOps

`frameworks/base`, `core/java/android/app/AppOpsManager.java`:

```java
public static final int OP_RECEIVE_SMS = AppOpEnums.APP_OP_RECEIVE_SMS;
public static final int OP_RECEIVE_EMERGECY_SMS = AppOpEnums.APP_OP_RECEIVE_EMERGENCY_SMS;

new AppOpInfo.Builder(OP_RECEIVE_EMERGECY_SMS, OPSTR_RECEIVE_EMERGENCY_BROADCAST,
        "RECEIVE_EMERGENCY_BROADCAST").setSwitchCode(OP_RECEIVE_SMS)
```

The test app passes the AppOp to `sendOrderedBroadcastAsUser`. `BroadcastSkipPolicy` then checks it
against the receiving manifest receiver via `noteOpForManifestReceiver`. AppOps is a real, additional
gate — not decoration.

## 9. Privileged / system applications

A privileged app is one on a priv-app partition *and* listed in a `privapp-permissions` allowlist;
that is what allows it to hold `signature|privileged` permissions. But **being privileged does not
make you a system UID.** `BroadcastController`'s protected-broadcast check accepts only `ROOT_UID`,
`SYSTEM_UID`, `PHONE_UID`, `BLUETOOTH_UID`, `NFC_UID`, `SE_UID`, `NETWORK_STACK_UID`, or a persistent
app. A priv-app running in its own UID still fails.

This is the single most commonly misunderstood point about the whole subject, and it is why the AOSP
test app uses `android:sharedUserId="android.uid.phone"` *in addition to* platform signing.

## 10. AOSP test application

| Attribute | Value |
| --- | --- |
| Package | `com.android.cellbroadcastreceiver.tests` |
| Location | `packages/apps/CellBroadcastReceiver/tests/testapp/` |
| Shared UID | `android.uid.phone` |
| Certificate | `platform` |
| `platform_apis` | `true` |
| Build module | `android_test` |
| Declared permissions | `BROADCAST_SMS`, `INTERACT_ACROSS_USERS_FULL` |
| Entry activity | `SendTestBroadcastActivity` (exported, LAUNCHER) |

It builds messages two ways: the **CMAS path** constructs `SmsCbMessage` directly; the **ETWS/generic
path** builds a raw hex PDU, patches serial number and identifier, and decodes it with its own copy of
`GsmSmsCbMessage`. Both then call `sendOrderedBroadcastAsUser` with an explicit package target.

## 11. Android 16

Primary reference. Commit `b97c8a4ffa3946d7206808bf4810746678b44a5c` (`android16-release`). Change
relative to 15: `CellBroadcastApp` gained `updatable: true`, so the app is delivered as a mainline
module. The injection mechanism is unchanged.

## 12. Android 15

Commit `62e355afa0062c10687f991a8ed7e0405f641003`. The test app's `sendBroadcast` helper is identical
to 16's.

## 13. Android 14

Commit `346bb742baaac29cc9509a39e9f9419647f994e7`. `CellBroadcastDefaults` does not exist as a shared
defaults block (fields are inline) and `updatable: true` is absent; everything relevant to the
injection path is identical.

## 14. user vs userdebug vs eng

| | user | userdebug | eng |
| --- | --- | --- | --- |
| Protected-broadcast + permission path | works | works | works |
| CBR `*#*#2627#*#*` testing mode | works if `allow_testing_mode_on_user_build` (AOSP: true) | works (`ro.debuggable==1`) | works |
| Channels marked `debug_build=true` | **dropped** | available | available |
| `test_cell_broadcast_receiver_packages` extra broadcast from CBS | not sent | sent | sent |
| Platform-signed test APK installable | **no** (no platform key) | yes | yes |

The build type matters for the *convenience* features and for whether a platform-signed APK can be
installed at all. It does not change the core permission model.

## 15–20. OEM differences (Pixel, Samsung, Xiaomi/HyperOS, Motorola, Nothing)

**No OEM had been inspected at the time of writing this report.** The AOSP tree does, however, ship
57 carrier overlays, and two of them demonstrate that OEM/carrier policy can change behaviour:

* `allow_testing_mode_on_user_build` is `true` in AOSP defaults and **`false`** in the Japan/docomo
  overlay (`values-mcc440-mnc20`) — so the testing-mode toggle can legitimately be unavailable.
* Four overlays use `testing_mode=true` channels (mcc284, mcc424, mcc450-mnc05, mcc450-mnc06); two use
  `debug_build=true` (mcc234, mcc440-mnc20).

Because the app is overlay-driven (`overlayable.xml`, `RROSampleTestApp/`), the correct strategy is to
validate on AOSP/Pixel and treat OEM divergence as configuration. Full status and per-OEM method are in
[`docs/oem-compatibility.md`](docs/oem-compatibility.md); per-OEM experiments are 10–14 in
[`docs/experiments.md`](docs/experiments.md).

## 21. Stock unrooted phones

**The mechanism is closed.** An ordinary APK cannot:

* send a protected broadcast (UID check, evaluated before any permission check),
* hold `RECEIVE_EMERGENCY_BROADCAST` (`signature|privileged`),
* share `android.uid.phone`,
* be signed with the platform key.

ADB does not help: shell is UID 2000, which is not in the accepted list, and no shell command
interface for injecting CB exists in either CB module (searched for `ShellCommand`, `onCommand`, and
any binder injection API — none found).

What a stock device *can* do is observe and configure, which is genuinely useful and is where the
experiments should start.

## 22. Rooted phones

Root is the first environment where the mechanism plausibly opens, because `ROOT_UID` is explicitly in
`BroadcastController`'s `isCallerSystem` list. Root also allows placing a helper into a priv-app
location and adding it to a `privapp-permissions` allowlist.

Remaining unknowns: AppOps state for the helper, SELinux policy, and whether a durable installation
survives updates. **Status: LIKELY, requires experimental verification.** These are Experiments 7 and
the reason they exist.

## 23. Development phones

A device we control at the system level is where the mechanism is **confirmed rather than likely**:
an AOSP userdebug/eng build, a GSI build, or a custom ROM. In that environment the test application is
part of the tree, already platform-signed, already in `android.uid.phone`, and installable. There is no
ambiguity and no invention required.

This is the recommended path, and it is the path the first proof of concept should use.

## 24. ADB

ADB is a **transport, not an authority**. Specifically:

* `am broadcast` of the protected action from shell is expected to fail with `SecurityException`.
  This is now backed by the shipping image: both CB actions are declared `<protected-broadcast>`.
* There is **no** `cmd`/`service` injection interface for CB.
* ADB **can** enumerate devices (`adb devices -l`), read logs (`logcat`), and start the test app's own
  exported activity via `am start`.

The hope was that the last point would make ADB a sufficient trigger. **Experiment 3 disproved that
for any shipping device**, for two independent reasons:

1. `CellBroadcastReceiverTests` is not present in a real AOSP image and not in any `PRODUCT_PACKAGES`.
   It ships on no build type. There is nothing for ADB to launch.
2. `SendTestBroadcastActivity` has no `onNewIntent` override and never calls `getIntent()`. It is a
   pure GUI. `am start` renders the UI and sends nothing; a UI tap is mandatory.

So ADB remains the right *transport*, but it cannot *initiate* the privileged operation. It can only
drive a test app that a custom build or a rooted device has already installed. The controller is a UI
driver plus a logcat observer, not a broadcaster.

The decisive device question therefore narrows from "can ADB send the broadcast?" (no) to
"can ADB drive the test app's UI once the test app exists?" — which remains Experiment 6.

## 25. Local testing

The entire design must work offline. Verified component by component: ADB/USB needs no internet; mDNS
is link-local; the injection path touches no network; the alert itself is entirely local. A system that
cannot reach the internet cannot accidentally cause a real broadcast through a network API — that is a
structural safety property, not a policy promise.

## 26. Wi-Fi controller possibilities

Feasible and straightforward once a device-side agent exists. Requirements: LAN-only binding, per-device
paired tokens, replay protection, explicit confirmation, audit logging, rate limiting. Full design in
[`docs/transport-options.md`](docs/transport-options.md) and
[`docs/security-and-safety.md`](docs/security-and-safety.md). Explicitly rejected: an unauthenticated
`POST /emergency` reachable by any LAN host.

## 27. USB controller possibilities

Recommended for the first proof of concept: deterministic enumeration, no discovery problem, ADB's own
key trust as authentication, and no half-delivery ambiguity. The constraint is the same as everywhere
else — the *agent* needs privilege; the cable does not provide it.

## 28. Multi-device control

Architecturally trivial (per-device fan-out over HTTP/WebSocket with per-device acknowledgement and
explicit state: `READY` / `BUSY` / `OFFLINE` / `UNPAIRED` / `UNSUPPORTED`). Target scale 3–10 devices.
Every command carries a unique id, retries are idempotent, and a partial fan-out must never be reported
as a global success. None of this is blocked by Android; it is blocked only by having devices to test
against.

## 29. Actual cellular transmission — investigated, not required

Producing a real Cell Broadcast requires a Cell Broadcast Centre, core-network and RAN configuration,
a base station (or an SDR acting as one), spectrum, and regulatory authorization. It is a
telecommunications-engineering and legal problem, not a software one.

**It is also unnecessary.** Everything the project actually wants — classification, settings,
database, alarm-stream sound, vibration, full-screen lock-screen UI, notification, DND interaction —
happens strictly *above* the broadcast boundary and reads nothing from the modem. Once equivalent
bytes reach that boundary, the experience is the genuine one.

Recorded here for completeness, and deliberately **not** pursued further: transmission on licensed
spectrum would be both illegal and outside the project's purpose.

## 30. Why ordinary apps cannot impersonate carrier emergency broadcasts

Because the platform is designed to prevent exactly that, in four independent places:

1. the two CB actions are `<protected-broadcast>`, and `BroadcastController` rejects any sender whose
   app id is not a system UID or a persistent app — *before* any permission check;
2. `RECEIVE_EMERGENCY_BROADCAST` is `signature|privileged`, so it is unavailable to ordinary apps;
3. `BroadcastSkipPolicy` verifies the sender holds the `receiverPermission` and that the AppOp is
   allowed, and separately verifies the *receiving manifest receiver* holds the permission (skipped
   only for `SYSTEM_UID` receivers);
4. `CellBroadcastReceiver` is `privileged: true` and signed with a platform-class certificate, so it
   cannot be replaced by a lookalike.

The correct conclusion is not "we need to get around this" but "this is why the tool must run on a
device we already control at the system level".

## 31. Legitimate test mechanisms

Android provides exactly one, and it is the project's golden path: the AOSP Cell Broadcast test
application, which injects a message at the CBS→CBR broadcast boundary using an identity that the
platform recognises as legitimate. Its mechanism is documented in
[`docs/aosp-test-path.md`](docs/aosp-test-path.md).

Two supporting mechanisms exist and are *not* alternatives:

* CBR's testing mode (`*#*#2627#*#*`), which unlocks `testing_mode=true` channels — but runs *after*
  the broadcast has already passed the security check;
* CBS's `test_cell_broadcast_receiver_packages` duplicate broadcast, which is debug-only and sits
  *downstream* of CBS, so reaching it still requires a modem-received message.

## 32. Security boundaries

| Boundary | Enforced by | Consequence |
| --- | --- | --- |
| sender identity for protected broadcasts | `BroadcastController` | ordinary apps and shell cannot inject |
| holder of `RECEIVE_EMERGENCY_BROADCAST` | package manager + privapp allowlist | injection requires a privileged, system-UID identity |
| AppOp | `BroadcastSkipPolicy` / `noteOpForManifestReceiver` | an additional user-visible gate |
| receiver integrity | platform signing of CBR | the receiver cannot be substituted |
| SELinux | kernel policy | may further restrict a hand-rolled helper; **UNKNOWN in detail** |
| alert dismissal | CBR internals (`DISMISS_DIALOG`, `dismiss()`) | no exported entry point found for third-party or remote dismissal |

## 33. Safety boundaries

Restated from [`docs/security-and-safety.md`](docs/security-and-safety.md), which is the authoritative
version:

1. Never transmit on cellular spectrum; never touch the modem path.
2. Never impersonate an authority, in the body text or anywhere else.
3. Never send silently — always an explicit, human confirmation.
4. Always label the alert as a test, using a genuine test category wherever the platform provides one,
   and a mandatory `TEST ALERT — SIMULATION` prefix where the body is under our control.
5. Never weaken Android's own safety behaviour to make a demo more dramatic.
6. Never use Presidential or AMBER classes for tests.
7. Keep the tool on controlled devices and a controlled network.

## 34. Feasibility matrix

| Goal | Stock unrooted | ADB only | Root | Root + system helper | AOSP userdebug/eng | Actual cellular |
| --- | --- | --- | --- | --- | --- | --- |
| Trigger a test alert | NOT POSSIBLE | NOT POSSIBLE | LIKELY | LIKELY | LIKELY (needs test APK built + installed) | works (real broadcast) |
| Genuine system UI | NOT POSSIBLE | NOT POSSIBLE | LIKELY | LIKELY | LIKELY | CONFIRMED |
| Genuine alert sound | NOT POSSIBLE | NOT POSSIBLE | LIKELY | LIKELY | LIKELY | CONFIRMED |
| Genuine vibration | NOT POSSIBLE | NOT POSSIBLE | LIKELY | LIKELY | LIKELY | CONFIRMED |
| PC control | n/a | viable transport | LIKELY | LIKELY | LIKELY | n/a |
| Wi-Fi control | n/a | n/a | LIKELY | LIKELY | LIKELY | n/a |
| Multiple phones | n/a | LIKELY | LIKELY | LIKELY | LIKELY | n/a |
| Works without internet | CONFIRMED | CONFIRMED | CONFIRMED | CONFIRMED | CONFIRMED | CONFIRMED |
| No cellular transmitter | CONFIRMED | CONFIRMED | CONFIRMED | CONFIRMED | CONFIRMED | NOT POSSIBLE |
| Cancel pending alert | n/a | LIKELY | LIKELY | LIKELY | LIKELY | n/a |
| Remotely dismiss displayed alert | NOT POSSIBLE | NOT POSSIBLE | UNKNOWN | UNKNOWN | UNKNOWN | NOT POSSIBLE |

### How to read this matrix

Two columns were downgraded from the Phase 1 draft after Experiment 3, and the reason matters:

* **"ADB only" is NOT POSSIBLE for triggering.** ADB is a transport, not an authority, and there is
  no longer any test app for it to drive. Experiment 3 confirmed that
  `CellBroadcastReceiverTests` ships on no build type and is absent from a real AOSP image. There is
  therefore no ADB-only path from a stock device to a genuine alert. This is a negative result, and it
  is the single most important correction in this report.
* **"AOSP userdebug/eng" was downgraded from CONFIRMED to LIKELY for *triggering*.** The path is
  confirmed *by construction* from source — the test app exists, is platform-signed, shares
  `android.uid.phone`, and its call site traces into `CellBroadcastReceiver`. But it has not been
  executed on hardware, and two things must hold at run time: the test APK must be built and installed
  deliberately (it is not in `PRODUCT_PACKAGES`), and the send must be driven through the activity's
  UI because there is no Intent-driven trigger. Neither has been observed. `LIKELY` is the honest
  label.

The "CONFIRMED" entries for the AOSP column are confirmed *by construction from AOSP source*, not by
execution. [`docs/experiments.md`](docs/experiments.md) states this explicitly, and Experiment 5
exists to convert them to observed results.


## 35. Recommended development sequence

1. Non-invasive reconnaissance on any available device (Experiments 1, 2).
2. Emulator/GSI question (Experiment 3).
3. **Highest value: does `am start` on the test activity produce a genuine alert?** (Experiment 6)
4. Build and install the test app from a matching AOSP tree on a userdebug/eng device (Experiment 4).
5. Prove the end-to-end path and the acceptance criteria (Experiment 5). **This is the project's
   decision point.**
6. Establish the minimal privilege set for a custom helper (Experiment 7) and settle CANCEL semantics
   (Experiment 9).

7. Only then build the smallest controller (Experiment 15).
8. Only after that: Wi-Fi transport, multi-device fan-out, device selection, logging, and UI.

---
# Addendum — Mission 2A: the genuine alert chain is proven

This addendum supersedes the previous milestone verdict. The earlier conclusion was that the chain
was *architecturally valid but unprovable* because AOSP's test APK ships on no build and its trigger
is GUI-only. That analysis was correct about the test APK and wrong about the mechanism. The test APK
is not the mechanism.

**The path is proven.** Android's own Cell Broadcast subsystems have now processed a locally
constructed test message on a live Android 15 (API 35) `userdebug` target, producing the genuine
alert experience. Raw evidence is in [`docs/experiments.md`](docs/experiments.md), EXP-ALERT-002
through EXP-ALERT-004.

## What was actually proven

```
AlertInjector (root, app_process)
  → constructs SmsCbMessage reflectively
  → broadcasts android.provider.action.SMS_EMERGENCY_CB_RECEIVED   [protected-broadcast]
  → CellBroadcastReceiver.onReceive                                [real, stock]
  → CellBroadcastAlertService.onStartCommand                       [real, stock]
  → shouldDisplayMessage / isChannelEnabled classification         [real, stock]
  → broadcasts table in cell_broadcasts_v13.db                     [real, stock]
  → CellBroadcastAlertAudio  → real alert sound → real TTS of the body
  → CellBroadcastAlertDialog → real on-screen alert, screen held awake
```

Every component named above is a stock, unmodified Android component, and every line of evidence
came from those components rather than from our tool.

| Stage | Verdict | Evidence |
| --- | --- | --- |
| A valid `SmsCbMessage` can be constructed locally | **CONFIRMED** | The tool built one and the framework accepted it. |
| Root can send the protected broadcast | **CONFIRMED** | `CellBroadcastReceiver.onReceive` logged the action with `(has extras)`. |
| Shell can send it | **NOT POSSIBLE** | `Permission Denial: not allowed to send broadcast ... uid=2000`. |
| The receiver forwards to the alert service | **CONFIRMED** | `CBAlertService: onStartCommand: android.provider.action.SMS_EMERGENCY_CB_RECEIVED`. |
| Classification and preference filtering run | **CONFIRMED** | Filtered first, then passed once `testing_mode` was set. |
| The alert reaches the real history database | **CONFIRMED** | Two rows: `4355\|1\|TEST ALERT - SIMULATION`. |
| The real alert dialog appears | **CONFIRMED** | `uiautomator` read `ETWS test message` / `TEST ALERT - SIMULATION`. |
| Real sound plays | **CONFIRMED** | `CellBroadcastAlertAudio: ALERT_SOUND_FINISHED`, audio focus taken and released. |
| Real TTS speaks the body | **CONFIRMED** | `CellBroadcastAlertAudio: Speaking broadcast text: TEST ALERT - SIMULATION`. |
| The screen is held awake | **CONFIRMED** | `added FLAG_KEEP_SCREEN_ON`, later removed. |
| No cellular transmission occurred | **CONFIRMED** | The emulator has no radio path in this flow; nothing touched `CbConfig` or the modem. |
| Vibration, lock screen, DND override | **NOT YET TESTED** | Carried forward. |

## The two corrections to Phase 1

**Correction 1 — the AOSP test APK is irrelevant.** The previous addendum concluded that the test
app's absence was a blocking precondition. It is not. `CellBroadcastReceiver` accepts its input as a
`SmsCbMessage` Parcelable in a broadcast extra, so any tool that can construct that object and clear
the protected-broadcast gate is equivalent. The test app is one such tool; ours is another.

**Correction 2 — root alone is sufficient, with one non-privilege prerequisite.** Phase 1 assumed
that `RECEIVE_EMERGENCY_BROADCAST` (a `signature|privileged` permission), AppOps, and SELinux would
each have to be satisfied. None of them apply to this path. The gate that matters is the
`<protected-broadcast>` UID check, and `ROOT_UID` passes it. The one thing root must *also* do is set
the receiving app's own preferences — `enable_test_alerts` and `testing_mode` — in its private
shared-prefs file, because test alerts are filtered off by default. That is a setting of the
receiving application, not a framework privilege.

The practical consequence is that a rooted retail device is a viable target, and an AOSP build is not
required. This is a materially cheaper path than Phase 1 predicted.

## The corrected smallest demo

The previous plan (`Windows → ADB → drive the AOSP test app GUI → real alert`) is replaced by:

```
PC
 → ADB
 → the device is root
 → ensure enable_test_alerts + testing_mode are set  (once)
 → push and run AlertInjector with an ETWS test message
 → REAL CellBroadcastReceiver → REAL alert UI + sound + TTS
```

This has been executed by hand and works. What remains is to wrap it in controller software. No AOSP
build, no platform signature, no system-app install, and no GUI automation of a test app.

## Cancellation — what changes in the design

`EXP-ALERT-004` settled the CANCEL question empirically:

* A displayed alert cannot be dismissed with BACK or with `CLOSE_SYSTEM_DIALOGS`. Both were tried and
  both failed. The alert screen is deliberately sticky, exactly as the security model implies.
* The only working dismissal is the alert's own button. A controller can reach it, but that is a
  visible on-device action, not a silent remote abort.
* The dialog queues messages: two sends produced `OK (1/2)`. Dismissal advances the queue rather than
  clearing it. Retry logic must account for this — a blind retry injects a second alert rather than
  replacing the first.

The controller therefore needs two distinct verbs, and must not conflate them:
**CANCEL PENDING** (ours, always safe) and **DISMISS ON DEVICE** (best-effort, user-visible).

## Verdict against the milestone

`We have established the mechanism and proven it end to end. We are ready for implementation.`

The remaining risk is not the Android mechanism. It is engineering: a controller that sets up the
preferences, invokes the injector over USB or Wi-Fi, handles multi-device fan-out, and never sends an
alert without explicit confirmation.


---

# Executive Conclusion

The Mission 1 sections above remain the reference description of Android's Cell Broadcast
architecture, its privilege model and its protocol details. This conclusion reflects what is now
known to be true after the device work.

## What we can definitely do

Verified by execution on a live Android 15 `userdebug` target, with raw logs in
`docs/experiments.md`:

* Construct a valid `SmsCbMessage` from our own code, with no AOSP build and no platform signature.
* Inject it into the genuine `CellBroadcastReceiver` from the root UID.
* Cause the real `CellBroadcastAlertDialog` to appear, with our own message text rendered verbatim
  under Android's own alert title.
* Cause real alert audio and real text-to-speech of the message body.
* Hold the screen awake for the duration of the alert.
* Have the alert recorded in the real `cell_broadcasts_v13.db` history.
* Control the alert type: the warning type was pinned to the ETWS test warning type, and the channel
  to the ETWS test channel `0x1103`.
* Confirm the negative case: the same tool run as shell UID is refused with
  `Permission Denial: not allowed to send broadcast`.
* Do all of the above with no cellular transmission of any kind.

## What appears possible

Plausible, not yet demonstrated on a device:

* Vibration for the test alert. The run logged `no pulsation pattern`; the pattern source for test
  alerts has not been exercised.
* The full-screen lock-screen presentation. The dialog sets the relevant flags, but the device was
  unlocked during our runs.
* A Do Not Disturb override, which is per-channel configuration.
* Wi-Fi control rather than USB/ADB.
* Multi-device fan-out from one controller.
* The same mechanism on Android 14 and Android 16. The API and the protected broadcast are stable
  across these versions, but only Android 15 has actually been tested.

## What requires root

* Sending the protected broadcast at all. This is the load-bearing requirement.
* Reading and writing the receiving app's private shared-prefs file, which is how `testing_mode` and
  `enable_test_alerts` get set.

An AOSP `userdebug`/`eng` build supplies root via `adb root`, so on a development build this costs
nothing extra.

## What requires userdebug / eng / custom Android

* **Nothing**, for the mechanism itself. This is the headline correction to Phase 1. Root is the
  requirement; how root is obtained is a deployment choice. `adb root` on a userdebug build is the
  easiest route, but a rooted retail device is sufficient.
* An AOSP build *would* be required if we wanted to use `CellBroadcastReceiverTests` as the tool, but
  it is no longer needed and no longer on the critical path.

## What stock Android cannot do

Note this means *unrooted*, which is the trap the original brief anticipated:

* An ordinary third-party APK cannot send the protected broadcast, cannot hold
  `RECEIVE_EMERGENCY_BROADCAST`, and cannot construct or deliver an `SmsCbMessage`. This remains
  absolutely closed.
* ADB alone, at shell UID, cannot do it. Proven, not inferred.
* Nothing on a stock unrooted device can trigger the genuine pipeline at all.

## What requires modem / cellular infrastructure

* A real Cell Broadcast reaching the device from a network. That is a completely different problem,
  requires 3GPP CBC/core-network/RAN infrastructure or a lab setup, and is out of scope. The sections
  above investigate it only to establish that it is not necessary for the genuine UI — which the
  device work has now confirmed.

## What we should NOT attempt

* Cellular transmission, at any point, for any reason. Unchanged, and now demonstrably unnecessary.
* Impersonating a carrier or government alert authority. Unchanged.
* Selecting a real hazard category. Our tool pins the warning type to the ETWS *test* warning type;
  there is deliberately no option to choose an actual emergency category.
* Blind retries. `EXP-ALERT-004` showed the dialog queues alerts, so a retry adds a second alert
  rather than replacing the first.
* Reworking the message to look more "realistic". The alert text is free-form and we can already make
  it say anything; the test nature is conveyed by the warning type and the channel, and that is the
  correct boundary. Do not blur it.
* Building the AOSP test app. It is unnecessary.

## Recommended Proof of Concept

Already executed manually; the task is to make it repeatable:

```
PC (any OS)
  → adb
  → device: root available
  → one-time setup: enable_test_alerts=true, testing_mode=true
  → push alertinject.jar
  → run AlertInjector 4355 "TEST ALERT - SIMULATION"
  → REAL Android emergency-alert UI + sound + TTS
```

The next milestone is a thin controller that performs exactly these steps, with an explicit
confirmation prompt and a dry-run mode, and nothing else.

## Recommended Final Architecture

```
              PC CONTROLLER
        [ SEND TEST ]   [ CANCEL PENDING ]
              │
              │  USB/ADB today, Wi-Fi later
              ▼
      per-device agent / adb bridge
        - verifies root
        - verifies testing_mode prefs
        - invokes AlertInjector with a fixed test warning type
        - reads logcat for the outcome
              │
    ┌─────────┼─────────┐
    ▼         ▼         ▼
 Android A  Android B  Android C
    ▼         ▼         ▼
 CellBroadcastReceiver (stock, unmodified)
    ▼
 REAL system alert UI + sound
```

The transport layer is entirely separate from the alert processing layer. The controller holds one
safety-critical invariant: it only ever produces ETWS test-type messages, and it requires explicit
confirmation before each send.

## Exact Next OpenHands Task

**Mission 2 delivered this task.** The controller exists and is verified. The next task is
`EXP-ALERT-002`.

> **Task: verify the alert experience on a locked screen.**
>
> 1. Boot the emulator first (`emulator -avd test35 -no-window -no-audio -accel off`, ~9 minutes).
> 2. Lock the device: `adb shell input keyevent KEYCODE_SLEEP`.
> 3. Send an alert with the existing controller: `python3 tools/test_alert.py --yes`.
> 4. Capture whether `CellBroadcastAlertDialog` appears over the lock screen, and record the exact
>    window state from `dumpsys window`.
> 5. Check vibration: `adb logcat -d | grep -i vibra`. If the emulator reports no pulsation pattern,
>    record that as an emulator limitation and mark vibration UNKNOWN rather than claiming success.
> 6. Attempt a DND override: set Do Not Disturb, send again, and observe whether the alert still
>    sounds.
> 7. Append the results to `docs/experiments.md` as `EXP-ALERT-002`, update `progress.md`, commit.
>
> Do not build Wi-Fi, multi-device fan-out or remote dismissal. Those are later milestones.

---

# Addendum — Mission 2: the controller is built

The proof of concept is now a usable application. `tools/test_alert.py` and the Tkinter GUI
(`app/main.py`) both drive the same controller, and CI produces a real Windows executable.

## What was built

| Component | Path | Role |
| --- | --- | --- |
| ADB layer | `app/adb.py` | Argument-array subprocess calls; device discovery and readiness assessment |
| Controller | `app/controller.py` | Validation, preparation, injection, evidence-based result detection |
| Models | `app/models.py` | Device states, alert states, failure codes |
| Logging | `app/logsetup.py` | Persistent local log |
| CLI | `tools/test_alert.py` | The control engine with no GUI |
| GUI | `app/ui.py`, `app/main.py` | Tkinter desktop interface |
| Packaging | `packaging/` | PyInstaller spec and Windows build script |
| CI | `.github/workflows/build-windows.yml` | Produces `Emergency-Simulator.exe` |
| Tests | `tools/test_controller.py`, `tools/test_ui.py` | Safety invariants; real widget tree |

## End-to-end result

Against `emulator-5554` (Android 15 / API 35, userdebug), a controller-invoked send produced:

```
CellBroadcastReceiver.onReceive          -- message accepted by the receiver
CellBroadcastAlertService.onStartCommand -- alert service started
CellBroadcastAlertAudio                  -- genuine alert audio engaged
CellBroadcastAlertDialog                 -- genuine full-screen alert displayed
```

and a row in the genuine history database (`7|4355|1|TEST ALERT - SIMULATION`).

## The finding that justifies the design

`adb shell` flattens its arguments into a string the device's shell re-parses. A multi-word body was
silently truncated to `TEST`: the injector exited 0, the genuine alert appeared, and nothing anywhere
reported an error. Only the history database revealed it. The fix is `shlex.quote` on every remote
argument, and the lesson is that **a clean exit code is not evidence**. The controller therefore
decides success from the production components in logcat, and a result is never reported as success on
the injector's exit status alone.

## Safety, as implemented

* The service category is a module constant, `4355` (0x1103). No parameter, config key, API field or
  CLI flag can change it; tests assert both the value and the absence of any hazard-selecting option.
* Bodies must begin with `TEST` and are validated before any device is touched.
* The CLI requires an explicit `y`; the GUI requires a confirmation dialog that repeats target,
  build, type, locked channel and message, defaulting to "no".
* `prepare_test_mode` refuses to act when a preference read fails, rather than guessing, because the
  secret code is a toggle and a wrong guess could disable a working configuration.
* The dry run was verified to leave the device's preference file byte-identical.
* There is no modem, radio or RF path, and no scheduling or retry.

## CANCEL

Before delivery, STOP cancels the pending local operation. After delivery it reports that Android
does not permit remote dismissal and points at the alert's own on-device control. This reflects
EXP-ALERT-004: BACK is swallowed by an `OnBackInvokedCallback` the alert window registers
deliberately, and `CLOSE_SYSTEM_DIALOGS` is ignored. No bypass is attempted.

## What is still not established

* `EXP-ALERT-002`: lock-screen presentation, vibration and DND override. The device was never locked,
  so these remain unverified.
* Vibration in particular was logged as `no pulsation pattern` on the emulator, so the emulator may
  not be able to demonstrate it at all.
* Android 14 and 16 are inferred, not tested. 15 is verified.
* Rooted retail devices via `su`: the code path exists and is unit-exercised, but has not been run
  against real hardware.

## Still out of scope

Not because they are hard, but because they come after the mechanism is proven:

* Wi-Fi control, an HTTP server, phone-to-phone control
* multi-device fan-out and device selection
* remote dismissal of a displayed alert (shown not to work; see `EXP-ALERT-004`)
* a cloud backend, accounts, or wide-area deployment

The milestone was one Windows PC reliably controlling one rooted Android test device, and that is
delivered.

---

# Addendum — Mission 2B: the release is hardened and self-verifying

## What changed

Mission 2 produced a working controller and an executable that CI could build. Mission 2B made that
release something that can be handed to someone else, and made a build that cannot work fail loudly
instead of shipping.

The gap that mattered was not functional. It was that **a build could succeed and still be unable to
find its own resources** — the defect recorded as BUG-004, where the frozen executable derived its
paths from an assumption about the packaging mode rather than from what the bundle actually
contained. That is now impossible to ship by accident.

## The release is self-contained

The executable carries:

* **The Android injector**, committed at `android/alertinject/out/alertinject.jar`. It was previously
  a gitignored build output of the Android SDK, which meant a fresh clone could not produce a working
  release without first building it with a JDK and the SDK. Committing one 3 KB artifact is a smaller
  price than a release that cannot drive a device, and the build now **fails** if it is absent rather
  than producing a binary that does nothing.
* **Android Platform Tools**, staged at build time and verified to include `adb.exe` together with
  `AdbWinApi.dll` and `AdbWinUsbApi.dll`. Those DLLs matter: without them `adb.exe` fails at load
  time, which is indistinguishable from "no adb" to any check that only tests whether a file exists.
  This is why discovery executes `adb version` instead.

Both are located through one module, `app/runtime.py`, so the controller, the CLI and the GUI cannot
disagree about where anything is.

## A build now verifies itself

`Emergency-Simulator.exe --selftest[=report.txt]` reports the search roots, the injector it resolved,
the adb it selected and that adb's version. It touches no device.

The build script and CI run it **from a directory that is not the repository**, on the real artifact,
and fail unless the injector is `FOUND` and the adb is `BUNDLED`. This is deliberately more than an
existence check, because the failure mode being guarded against is precisely a build that exists and
does not work. A windowed Windows build has no console, so the report can be written to a file and
read from there.

Verified locally on a onefile build launched from an empty directory with no `ADB_PATH`:

```
  frozen      : True
  bundle root : /tmp/_MEI00004316fqxBqu
  injector    : FOUND  /tmp/_MEI.../android/alertinject/out/alertinject.jar  (3385 bytes)
  bundled adb : /tmp/_MEI.../platform-tools/adb
  adb         : BUNDLED
  adb version : Android Debug Bridge version 1.0.41
RESULT: OK
```

That process then discovered the live emulator and logged to the per-user path, with no Python, no
SDK and no JDK on the machine.

And confirmed on the real target platform, by CI run `35449434829` on `windows-latest`, against the
artifact that is uploaded:

```
  frozen      : True
  bundle root : C:\Users\RUNNER~1\AppData\Local\Temp\_MEI000023142
  injector    : FOUND  C:\...\_MEI000023142\android\alertinject\out\alertinject.jar  (3385 bytes)
  bundled adb : C:\...\_MEI000023142\platform-tools\adb.exe
  adb         : BUNDLED
  adb version : Android Debug Bridge version 1.0.41
RESULT: OK
```

`adb: BUNDLED` on Windows also proves the companion DLLs were staged correctly, since `adb.exe` fails
at load time without `AdbWinApi.dll` and `AdbWinUsbApi.dll`.

## CI found real defects, twice

The first two runs of the revised workflow failed, and both failures were genuine — neither would
have been caught by the local Linux test run:

1. **The adb probe test asserted a POSIX-only error string.** It expected the failure text to contain
   exit status `127`, which a Windows exit code cannot produce. The property under test is that a
   present-but-unusable executable is rejected and the reason reported; the reason is
   platform-specific, so only the POSIX branch may assert it.
2. **The self-test verification read its report before the executable wrote it** (BUG-010). A
   windowed PyInstaller build is a GUI-subsystem binary, so PowerShell does not wait for it: the check
   read the file in the same breath as starting the process, then failed with "cannot find path"
   *before* the "report written" line appeared. `Start-Process -Wait -PassThru` is required.

The second is worth dwelling on. The verification was written specifically to catch BUG-004 — a build
that cannot find its own resources — and was itself broken from the moment it was written. On Linux
the binary is a console executable that does block, so it passed locally and failed only on Windows,
the one platform it existed for. It is recorded as BUG-010 with that observation, because a check that
cannot fail for the right reason is not a check.

## The interface is an instrument

The desktop window was rebuilt so it cannot be mistaken for, or overwritten by, the thing it tests.

Status is carried by a word as well as a colour, and no glyph is load-bearing, so the interface is
correct on a host without an emoji font and readable in monochrome. Devices appear as rows with a
support verdict and the reason behind it, derived from observed capability rather than from the
manufacturer — asserted by a test that five different brands with identical capability classify
identically.

Two safety properties are new and both are asserted by test:

* **The safety statement is permanent.** It states plainly that this is a test on a controlled device
  with no cellular transmission. Results announce themselves on a separate strip below it, so the one
  line that must always be true can never be replaced by a transient one.
* **SEND requires both a usable device and a clear send gate.** Either alone is not enough. A non-root
  device disables it and shows why, rather than refusing silently.

## The duplicate-send problem

Android queues emergency alerts and deliberately provides no way for software to withdraw or dismiss
a displayed one. A second send therefore does not retry anything: it stacks another full-screen
dialog with sound, which a person then has to dismiss individually.

The interface prevents this with a per-device gate — `READY`, `BUSY`, `DELIVERED`, `UNCERTAIN` — and
only an explicit **Acknowledge**, after the operator has dismissed the alert on the device, returns it
to `READY`. Because the alert's presence cannot be observed by a third party, it is remembered
instead, and clearing it is a human act. Verified on the emulator: a first send reaches
`ALERT_DISPLAYED` and sets `DELIVERED`, SEND disables itself, and a second send is refused with
`DUPLICATE_SEND_BLOCKED` before the device is touched.

The subtle part is `UNCERTAIN`. A timeout is not a failure: it means the outcome is unknown, and the
device may be showing an alert at that moment. Treating it as "nothing happened" is exactly the
reasoning that leads to a stacked alert, so it gates further sends just like `DELIVERED` does.

## The bug journal

`docs/bugs/` records every real defect with its reproduction, raw output, root cause and the test
that guards it. Two of them were **false successes** — a truncated alert body that exited 0, and a
secret code that silently toggled test mode off — where every signal the tool had said the operation
had worked. They are written down for that reason: they are the argument for deciding outcomes from
device evidence rather than from exit codes.

## What is still not established

Unchanged from Mission 2, and stated here so it is not mistaken for finished work:

* `EXP-ALERT-002`: lock-screen presentation, vibration and DND override. Still unverified, still
  because the device was never locked.
* Vibration in particular may not be demonstrable on the emulator at all.
* Android 14 and 16 remain inferred from stable APIs; 15 is verified.
* Rooted retail devices via `su` remain untested against real hardware.
* **The Windows artifact has not been driven against a device on Windows.** CI has no device, so the
  build is verified to resolve its resources and to start, but the Windows adb transport has not been
  exercised end to end. The driving code is identical Python and was exercised on Linux.

## Verdict

The release is now something that can be handed over: one executable, no dependencies, and a build
that proves the artifact works before it is published — on the platform it targets, with CI catching
two genuine defects that the local Linux run could not. What remains is not controller engineering but
the open Android questions above, and the deferred transports — Wi-Fi and multi-device — which the
controller is already structured to accept.