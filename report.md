# Feasibility investigation report

**Subject:** Can a PC or controller phone cause Android test devices to experience a *genuine* Android
Cell Broadcast emergency alert through Android's own emergency-alert machinery?

**Phase:** 1 — source analysis. No device has been used yet; every claim below is derived from AOSP
source or the framework's own security code, with the repository and commit recorded in
[`docs/sources.md`](docs/sources.md).

**Method:** clone and read the real components, trace the real execution path, and quote the decisive
code. Unknowns are marked `UNKNOWN` rather than guessed.

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
* There is **no** `cmd`/`service` injection interface for CB.
* ADB **can** enumerate devices (`adb devices -l`), read logs (`logcat`), and — most importantly —
  **start the test app's own exported activity** via `am start`. If that works, the activity performs
  the privileged broadcast *as itself*, and ADB becomes a viable trigger with no custom privilege
  work at all.

**That last point is the single highest-value unknown in the project.** It is Experiment 6.

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
| Trigger a test alert | NOT POSSIBLE | NOT POSSIBLE | LIKELY | LIKELY | **CONFIRMED** | works (real broadcast) |
| Genuine system UI | NOT POSSIBLE | NOT POSSIBLE | LIKELY | LIKELY | **CONFIRMED** | CONFIRMED |
| Genuine alert sound | NOT POSSIBLE | NOT POSSIBLE | LIKELY | LIKELY | **CONFIRMED** | CONFIRMED |
| Genuine vibration | NOT POSSIBLE | NOT POSSIBLE | LIKELY | LIKELY | **CONFIRMED** | CONFIRMED |
| PC control | n/a | viable transport | LIKELY | LIKELY | **CONFIRMED** | n/a |
| Wi-Fi control | n/a | n/a | LIKELY | LIKELY | LIKELY | n/a |
| Multiple phones | n/a | LIKELY | LIKELY | LIKELY | LIKELY | n/a |
| Works without internet | CONFIRMED | CONFIRMED | CONFIRMED | CONFIRMED | CONFIRMED | CONFIRMED |
| No cellular transmitter | CONFIRMED | CONFIRMED | CONFIRMED | CONFIRMED | CONFIRMED | NOT POSSIBLE |
| Cancel pending alert | n/a | LIKELY | LIKELY | LIKELY | LIKELY | n/a |
| Remotely dismiss displayed alert | NOT POSSIBLE | NOT POSSIBLE | UNKNOWN | UNKNOWN | UNKNOWN | NOT POSSIBLE |

"CONFIRMED" for the development-build column means *confirmed by construction from AOSP source*: the
test application exists in the tree, is platform-signed, shares `android.uid.phone`, and its call site
is traced into `CellBroadcastReceiver`. It has not yet been executed on hardware, and
[`docs/experiments.md`](docs/experiments.md) says so explicitly.

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

# Executive Conclusion

## What we can definitely do

Confirmed from AOSP source:

* **Identify the exact injection point.** `Intent(ACTION_SMS_EMERGENCY_CB_RECEIVED)` +
  `putExtra("message", SmsCbMessage)` + `setPackage(<CBR package>)`, sent as an ordered broadcast.
  This is where the real pipeline begins and where the test app enters it.
* **Confirm that everything downstream is genuine.** `CellBroadcastReceiver` →
  `CellBroadcastAlertService` → `CellBroadcastAlertAudio` (sound + vibration) and
  `CellBroadcastAlertDialog` (full-screen, lock-screen-capable) → `CellBroadcastContentProvider`.
* **Confirm that the modem is irrelevant to the user-visible result.** No component in that chain
  reads radio state.
* **Confirm the exact privileges the injector needs:** a system UID (platform signing plus a shared
  system UID, in practice) *and* a `signature|privileged` permission *and* an allowed AppOp.
* **Confirm that no stock, unrooted device and no ADB-only path can do this.**
* **Confirm the alert body is free-form**, and that the title is Android's own (overlayable).
* **Confirm that no "rocket attack" alert class exists**, and that ETWS has a genuine, first-class test
  warning type that Android itself labels as a test. The closest thing Android has is the CMAS
  **category** `CMAS_CATEGORY_CBRNE` = 0x0a, which Android itself renders as the string
  "Chemical/Biological/Nuclear/Explosive" under an "Alert Category:" heading.
* **Confirm which channels are safe and available by default** (`0x1103` ETWS test, `0x111C` CMTS
  monthly test), and that a channel with no configured range is silently dropped.
* **Build a fully offline, multi-device controller architecture** that cannot cause cellular
  transmission by construction.

## What appears possible

Plausible, requiring device experiments:

* Rooted devices can satisfy the remaining gates (AppOps, SELinux) and inject the broadcast.
* `am start` of the AOSP test activity from ADB produces a genuine alert — which would make an
  ADB-driven proof of concept very simple. **Highest-value unknown.**
* An AOSP/GSI emulator hosts the full pipeline.
* OEM devices that ship the unmodified AOSP receiver behave identically for the injection path.
* Cancel-pending is straightforward; remote dismissal is not.

## What requires root

* Injecting the protected broadcast from a non-platform process on a retail build.
* Installing a durable privileged helper outside the system image.
* Any work on a device whose bootloader we cannot unlock but which we can root.

## What requires userdebug / eng / custom Android

* Installing the platform-signed AOSP test application (needs the matching platform key).
* Channels configured `debug_build=true`.
* CBS's debug-only `test_cell_broadcast_receiver_packages` duplicate broadcast.
* The clean, unambiguous end-to-end proof of concept.

## What stock Android cannot do

* Send the CB emergency broadcast from an app or from ADB shell.
* Hold `RECEIVE_EMERGENCY_BROADCAST`.
* Install the AOSP test application.
* Display an alert on a channel with no configured range.
* Dismiss a displayed emergency alert remotely.
* Override DND for a test-class alert.

## What requires modem / cellular infrastructure

Only a *real* Cell Broadcast. Everything the project wants does not.

## What we should NOT attempt

* Any cellular transmission, on any spectrum, ever.
* Impersonating a government, agency, carrier, or real event — including in the free-form body text.
* Silent or one-click alerts.
* Building a fake emergency UI and calling it a success.
* Using Presidential or AMBER classes for tests.
* Weakening Android's own safety behaviour for a better demo.
* Presenting "root on someone else's phone" as a supported configuration.

## Recommended Proof of Concept

The smallest thing that proves the concept:

```
Hardware  : 1 PC + 1 Android device running an AOSP userdebug/eng build
            (or an emulator, if Experiment 3 succeeds)
Software  : the AOSP `CellBroadcastReceiverTests` APK, built from the matching tree
Sequence  : PC script -> adb -> launch SendTestBroadcastActivity / trigger one ETWS test message
            -> genuine full-screen alert with sound and vibration
Evidence  : logcat showing CellBroadcastReceiver -> CellBroadcastAlertService ->
            CellBroadcastAlertDialog, plus the row in Android's own CB history
Guarantee : the chosen channel is ETWS test (0x1103) and the body begins "TEST ALERT — SIMULATION"
```

If, and only if, that works, the same broadcast can be driven by a purpose-built device-side agent and
then by a network controller.

**The closest legitimate alternative if it does not:** a rooted device with an installed privileged
helper (Candidate B in [`docs/feasibility.md`](docs/feasibility.md) §4). The gap would be an explicit
privilege requirement, not a different mechanism.

## Recommended Final Architecture

```
                    ┌─────────────────────────────┐
                    │        PC CONTROLLER        │
                    │  [SEND TEST ALERT] [CANCEL] │
                    │  alert type / message /     │
                    │  target selection           │
                    └──────────────┬──────────────┘
                                   │  local only, authenticated, offline-capable
              ┌────────────────────┼────────────────────┐
              ▼                    ▼                    ▼
      ┌───────────────┐    ┌───────────────┐    ┌───────────────┐
      │  DEVICE AGENT │    │  DEVICE AGENT │    │  DEVICE AGENT │
      │  (privileged) │    │  (privileged) │    │  (privileged) │
      └───────┬───────┘    └───────┬───────┘    └───────┬───────┘
              │                    │                    │
              ▼                    ▼                    ▼
      CellBroadcastReceiver.onReceive()   <- the REAL receiver
              │                    │                    │
              ▼                    ▼                    ▼
      genuine CellBroadcastAlertService / AlertAudio / AlertDialog
              │                    │                    │
              ▼                    ▼                    ▼
        REAL ALERT            REAL ALERT           REAL ALERT
      sound+vibrate+UI      sound+vibrate+UI     sound+vibrate+UI
```

Marked clearly: **this is a proposed architecture, not a confirmed implementation.** The device agent
is the only part whose feasibility is still open, and it is open only with respect to *which*
environment supplies the privilege.

## Exact Next OpenHands Task

> Run **Experiment 1**, **Experiment 2**, **Experiment 3** and **Experiment 6** from
> [`docs/experiments.md`](docs/experiments.md) against whatever Android hardware or emulator is
> available. Record the raw command output for each. Do not write application code. Do not build the
> GUI. Do not implement the transport.
>
> Experiment 6 is the decisive one: determine whether
> `adb shell am start -n com.android.cellbroadcastreceiver.tests/.SendTestBroadcastActivity`
> (or an equivalent instrumentation invocation) can produce a genuine alert on a device that has the
> test app installed — and if the test app is not installed, record that fact together with
> `adb shell pm list packages | grep cellbroadcast`.
>
> Then update `docs/experiments.md` with actual results, update `progress.md`, and commit.

If no Android hardware is available, the next task instead is a **pure source-analysis task**: trace
`GsmCellBroadcastHandler` in CBS line by line to determine how `SmsCbEtwsInfo.getWarningType()` is
populated for identifier `0x1103` on the production decode path, and resolve open question 25/26 in
[`docs/open-questions.md`](docs/open-questions.md).
