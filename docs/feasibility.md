# Feasibility

This document gives the direct answer: **can it be done, how, under what conditions, and what are the
limitations.** Every cell is justified by `privilege-model.md`, `aosp-test-path.md`, and
`android-version-compatibility.md`.

## 1. Executive answer

**Yes — the core mechanism exists and has now been demonstrated on a live device.** Android's
genuine Cell Broadcast subsystems processed a locally constructed test message end to end: real
channel classification, real user settings, the real history database, real alarm audio, real
text-to-speech, and the real `CellBroadcastAlertDialog`. See `docs/experiments.md`,
EXP-ALERT-002.

There are two ways in, and they are not the same:

1. **The receiver's exported entry point.** `CellBroadcastReceiver` accepts an `SmsCbMessage` under
   the `"message"` extra of `android.provider.action.SMS_EMERGENCY_CB_RECEIVED`. That action is a
   `<protected-broadcast>`, so `ActivityManagerService` rejects any sender whose UID is not one of a
   short list of system UIDs (`ROOT_UID`, `SYSTEM_UID`, `PHONE_UID`, …) or a persistent app. Root
   passes this check. **This is the path that has been proven to work**, and it needs no AOSP build,
   no platform signature and no system-app install.
2. **AOSP's `CellBroadcastReceiverTests` app.** It also exists and exercises the same receiver, but
   it clears the gate by being platform-signed, sharing `android.uid.phone`, and building with
   `platform_apis: true`. It is a convenience, not the mechanism, and it cannot be built or
   installed in this environment.

**So the project is feasible, but it requires a device we control at the system level**: an
AOSP/userdebug development build, or a rooted device. On top of that the receiving app's
`enable_test_alerts` and `testing_mode` preferences must be set, or the message is dropped after
every permission check succeeds. Everything above that — the PC controller, USB/Wi-Fi transport,
multi-device fan-out, cancellation, logging — is ordinary software engineering and is not the risk.

## 2. Feasibility matrix

Legend: **CONFIRMED** (verified from AOSP source), **CONFIRMED (device)** (reproduced on a live
Android 15 target, see `docs/experiments.md`), **LIKELY** (strongly implied, needs a device test),
**UNKNOWN** (not determined), **NOT POSSIBLE** (closed by source), **REQUIRES DEV BUILD** /
**REQUIRES ROOT**.

| Goal | Stock Android (unrooted) | ADB only | Root | Root + system helper | Userdebug/eng AOSP | Actual cellular |
| --- | --- | --- | --- | --- | --- | --- |
| Trigger a *test* alert message | **NOT POSSIBLE** | **NOT POSSIBLE** | **CONFIRMED (device)** | **CONFIRMED (device)** | **CONFIRMED** | works but is a real broadcast |
| Genuine system UI (full screen) | **NOT POSSIBLE** | **NOT POSSIBLE** | **CONFIRMED (device)** | **CONFIRMED (device)** | **CONFIRMED** | **CONFIRMED** |
| Genuine alert sound (alarm stream) | **NOT POSSIBLE** | **NOT POSSIBLE** | **CONFIRMED (device)** | **CONFIRMED (device)** | **CONFIRMED** | **CONFIRMED** |
| Genuine TTS of the alert body | **NOT POSSIBLE** | **NOT POSSIBLE** | **CONFIRMED (device)** | **CONFIRMED (device)** | **CONFIRMED** | **CONFIRMED** for enabled channels |
| Alert written to the real history DB | **NOT POSSIBLE** | **NOT POSSIBLE** | **CONFIRMED (device)** | **CONFIRMED (device)** | **CONFIRMED** | **CONFIRMED** |
| Genuine vibration | **NOT POSSIBLE** | **NOT POSSIBLE** | **LIKELY** | **LIKELY** | **CONFIRMED** | **CONFIRMED** |
| Screen wake / show over lock screen | **NOT POSSIBLE** | **NOT POSSIBLE** | **LIKELY** | **LIKELY** | **CONFIRMED** (dialog sets `FLAG_SHOW_WHEN_LOCKED` + `FLAG_TURN_SCREEN_ON`) | **CONFIRMED** |
| DND override | **NOT POSSIBLE** | **NOT POSSIBLE** | **LIKELY** | **LIKELY** | **LIKELY** (only for channels configured `override_dnd=true`, or the global `override_dnd` setting) | **CONFIRMED** for configured channels |
| PC control | n/a | **CONFIRMED possible as a transport** | **CONFIRMED (device)** | **CONFIRMED (device)** | **CONFIRMED** | n/a |
| Wi-Fi control | n/a | n/a | **LIKELY** | **LIKELY** | **LIKELY** | n/a |
| Multiple phones | n/a | **LIKELY** (manual per-serial) | **LIKELY** | **LIKELY** | **LIKELY** | n/a |
| Works without internet | n/a | **CONFIRMED** | **CONFIRMED** | **CONFIRMED** | **CONFIRMED** | **CONFIRMED** |
| No cellular transmitter involved | **CONFIRMED** (nothing can happen) | **CONFIRMED** | **CONFIRMED (device)** — reproduced with no modem participation | **CONFIRMED (device)** | **CONFIRMED** | **NOT POSSIBLE** — this is the transmitter |
| Cancel *pending* alert | n/a | **CONFIRMED** — nothing is queued until the command is issued | **CONFIRMED** | **CONFIRMED** | **CONFIRMED** | n/a |
| Remotely dismiss an *already displayed* system alert | **NOT POSSIBLE** | **NOT POSSIBLE** | **NOT POSSIBLE** (device) — BACK and `CLOSE_SYSTEM_DIALOGS` are both swallowed; only the alert's own button works | **NOT POSSIBLE** | **NOT POSSIBLE** | **NOT POSSIBLE** |
| Custom alert *text* | **NOT POSSIBLE** | **NOT POSSIBLE** | **CONFIRMED (device)** — `SmsCbMessage` carries a free-form body; our text rendered verbatim | **CONFIRMED (device)** | **CONFIRMED** | **CONFIRMED** |
| A dedicated "rocket attack" alert class | **NOT POSSIBLE** | — | — | — | **NOT POSSIBLE** — no such identifier exists in Android | **NOT POSSIBLE** |

### 2.1 What the device run changed

Two Phase 1 verdicts were too pessimistic and have been corrected upward:

* **Root is sufficient for the whole injection.** The earlier analysis assumed the
  `signature|privileged` `RECEIVE_EMERGENCY_BROADCAST` permission, AppOps and SELinux would each
  have to be satisfied separately. In practice the protection that matters is the
  `<protected-broadcast>` UID check in `ActivityManagerService`, and `ROOT_UID` passes it. No
  manifest permission, no AppOps grant and no SELinux change was needed. The extra gates apply to
  *receiving* emergency broadcasts from the framework, not to *injecting* one into the receiver
  app's own exported entry point.
* **The AOSP test application is not required.** Any tool that can construct an `SmsCbMessage` and
  send the protected broadcast is equivalent for this purpose. The test app is a convenience, not
  the mechanism.

Three things remain genuinely unresolved and are carried forward as experiments: vibration, the
lock-screen presentation, and the DND override. The alert we produced did log `no pulsation pattern`,
which is expected for a test alert and means the vibration pattern source for the test path has not
yet been exercised.

### 2.2 The one prerequisite root must also handle

Root alone was **not** sufficient on our OEM-flavoured target, because the receiving app's own
preferences filter test alerts off by default. `enable_test_alerts` and `testing_mode` both had to be
set in the app's private preference file. This is a receiving-app setting rather than a framework
privilege, so it is reachable by root on any device — but any controller must establish it before the
first send, or the alert will be silently dropped after passing every permission check.

## 3. What each environment can realistically deliver

### 3.1 Stock, unrooted retail device

Nothing. Not "hard" — closed. An ordinary APK:

* **cannot** hold `RECEIVE_EMERGENCY_BROADCAST` (`signature|privileged`),
* **cannot** send a protected broadcast (UID check happens *before* permission checks),
* **cannot** share `android.uid.phone`,
* **cannot** be signed with the platform key.

The only thing a stock device can do is *observe and configure*: read the Cell Broadcast settings,
read the alert history, and change the user-visible toggles. That is genuinely useful for
Experiment 1 and Experiment 2, and it is where the investigation should start because it is entirely
non-invasive.

### 3.2 ADB on a stock device

ADB gives shell (UID 2000), which is not in the accepted system-UID list. `am broadcast` from shell for
`android.provider.action.SMS_EMERGENCY_CB_RECEIVED` is expected to throw a `SecurityException`.

ADB's real value is **as a transport to something else**: enumerating devices, reading settings,
reading logs (`logcat`) to prove what the receiver did, and invoking a privileged helper later.

No shell command interface for injecting CB messages was found in either the CellBroadcastService or
the CellBroadcastReceiver modules (searched for `ShellCommand`, `onCommand`, and any binder injection
API). That absence is itself an important negative result: it means there is no "official ADB hook"
to use, and a helper must be supplied.

### 3.3 Rooted device

Root is the first environment where the mechanism plausibly opens, because `ROOT_UID` is explicitly in
the `isCallerSystem` switch. Root also makes it possible to install a helper into a priv-app location
(satisfying Gate 3's partition requirement) and to add it to a `privapp-permissions` allowlist.

The remaining unknowns are AppOps and SELinux. Both are experiments, not assumptions.

### 3.4 AOSP userdebug/eng build — the recommended path

This is where the project is *confirmed* rather than *likely*:

* the test application is a first-class part of the AOSP tree (`tests/testapp`),
* building it produces an APK already signed with the platform key and already in
  `android.uid.phone`,
* it can be installed alongside the rest of the image with no hacks,
* `ro.debuggable == 1` unlocks `debug_build=true` channels,
* CBR's `allow_testing_mode_on_user_build` is `true` in AOSP defaults, so testing mode is reachable.

There is no ambiguity in this environment. The cost is hardware: it needs a device with an unlockable
bootloader and available AOSP or a GSI.

### 3.5 Actual cellular transmission

Fundamentally different, and out of scope by design. It requires core-network and RAN equipment (a
Cell Broadcast Centre, base station, subscriber/core configuration), regulatory authorization, and
spectrum. It is not a software problem and it is not needed: **nothing in the genuine alert UI, sound,
vibration, classification or database depends on the message having arrived over the air.** See §5.

## 4. The three candidate architectures, ranked

### Candidate A — AOSP userdebug/eng development device (recommended)

```
PC controller  --(ADB or Wi-Fi)-->  device agent (in-image, platform-signed, android.uid.phone)
                                        |
                                        v
                              CellBroadcastReceiver.onReceive -> genuine alert
```

* Feasibility: **CONFIRMED by construction.**
* Cost: one device with an unlockable bootloader + a build.
* Risk: low. The mechanism is the official one.
* This is the only architecture that needs no invention.

### Candidate B — Rooted retail device with an installed system helper

```
PC controller  --(ADB or Wi-Fi)-->  helper running as system/phone uid, holding
                                    RECEIVE_EMERGENCY_BROADCAST, allowlisted in privapp-permissions
                                        |
                                        v
                              CellBroadcastReceiver.onReceive -> genuine alert
```

* Feasibility: **LIKELY.**
* Cost: root, plus solving AppOps and SELinux.
* Risk: medium — depends on device and root method.
* Advantage: can use phones the operator already owns.

### Candidate C — Anything on stock without root

* Feasibility: **NOT POSSIBLE** for the genuine pipeline.
* The only honest fallback is a clearly-labelled *simulation* (our own notification/UI), which is
  explicitly **not** the project's objective and must never be presented as the real subsystem.

## 5. Why actual cellular transmission is not required for the goal

The genuine emergency experience is produced entirely inside `com.android.cellbroadcastreceiver`,
downstream of the broadcast boundary:

* classification: `CellBroadcastAlertService.openEmergencyAlertNotification` + `CellBroadcastChannelManager`
* sound: `CellBroadcastAlertAudio` (`USAGE_ALARM`, `STREAM_ALARM`, volume forced to maximum)
* vibration: `CellBroadcastAlertAudio` via `Vibrator.vibrate(effect, attrs)`
* UI: `CellBroadcastAlertDialog` (`FLAG_FULLSCREEN`, `FLAG_SHOW_WHEN_LOCKED`, `FLAG_TURN_SCREEN_ON`)
* persistence: `CellBroadcastContentProvider`

None of these read anything from the modem. The modem's only contribution is producing the bytes. Once
we can produce equivalent bytes at the broadcast boundary, the experience is identical.

## 6. What cannot be achieved

Recorded plainly so no future session re-litigates them:

1. **No ordinary APK can do this.** Closed by the protected-broadcast UID check.
2. **No ADB-only path exists.** Shell's UID is rejected; no shell injection interface exists in either
   CB module.
3. **No stock retail device, unmodified, can do this.** Platform signing + privileged permission.
4. **There is no "rocket attack" alert type in Android.** No identifier for it exists in
   `SmsCbConstants`. See `protocol-and-alert-types.md`.
5. **Emergency alert text cannot be made OEM-independent.** The *body* is free text we control, but the
   *title* is chosen by `CellBroadcastAlertDialog` from `res/values/strings.xml`
   (`cmas_presidential_level_alert` → "Presidential alert", `etws_earthquake_warning` → "Earthquake
   warning", etc.). OEMs overlay those strings.
6. **Remote dismissal of a displayed system alert is not supported** by any interface found so far.
   `DISMISS_DIALOG` and `CellBroadcastAlertDialog.dismiss()` are internal to the privileged app.
7. **A real broadcast cannot be produced without a real network.** No amount of software on the device
   can make the modem transmit a CB it did not receive from a base station.

## 7. What we should NOT attempt

* Do not transmit on licensed spectrum. Not even "a little".
* Do not attempt to make a stock phone pretend to be a carrier.
* Do not silently send test alerts — every send must be an explicit, confirmed, labelled action.
* Do not build a fake emergency UI and call it a success.
* Do not use a real alert identifier with real-looking wording outside a clearly-labelled test context,
  and never on a device that other people might mistake for a real warning.
* Do not disable or weaken Android's own safety behaviour (DND, notification, permission checks) to
  make the demo more dramatic.
* Do not push to a shared/production branch or repository without an explicit request.

## 8. The minimum viable proof of concept

```
Hardware: 1 PC + 1 device running an AOSP userdebug/eng build (or 1 emulator, if the emulator
          turns out to carry the CB apex and the test app — UNKNOWN, see Experiment 3)

Software: the AOSP `CellBroadcastReceiverTests` APK, built from the same tree as the device image,
          installed via `adb install`

Sequence: PC script -> adb -> launch the test activity / invoke one test method
          -> genuine alert appears with sound and vibration
          -> logcat shows CellBroadcastReceiver -> CellBroadcastAlertService -> CellBroadcastAlertDialog
```

Acceptance criteria:

1. The alert UI is Android's own `CellBroadcastAlertDialog`, not our view.
2. The sound comes from `res/raw/*.ogg` via `CellBroadcastAlertAudio`.
3. `logcat -s CellBroadcastReceiver CellBroadcastAlertService` shows the genuine component names.
4. The message appears in the Cell Broadcast history (`CellBroadcastContentProvider`).
5. The alert is visibly identifiable as a test (the chosen channel is a test channel).
6. The device's radio state is irrelevant to the result.

If criterion 1–3 hold, the central feasibility question is answered **yes** and the project moves to
implementation. If they do not, the fallback is Candidate B (rooted) and the gap is documented.
