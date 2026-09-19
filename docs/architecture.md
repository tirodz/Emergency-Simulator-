# Architecture

This document describes the Android Cell Broadcast / emergency-alert architecture as it actually
exists in AOSP, then places our proposed system on top of it. The first half is verified against
source; the second half is **proposed** and marked as such.

## 1. The real pipeline (verified, Android 16 era)

```
                        Radio / cellular layer
                    (base station broadcasts CB PDU)
                                  |
                                  v
                    +---------------------------------+
                    |  Modem / RIL                    |
                    |  unsolicited response carrying  |
                    |  a raw CB PDU                   |
                    +---------------------------------+
                                  |
                                  v
        frameworks/opt/telephony
        com.android.internal.telephony.CellBroadcastServiceManager
          - registers "onNewGsmBroadcastSms" with the phone
          - forwards raw bytes over AIDL:
              ICellBroadcastService.handleGsmCellBroadcastSms(slotId, byte[])
                                  |
                                  v
        packages/modules/CellBroadcastService      [mainline module, privileged]
        DefaultCellBroadcastService  (binds com.android.telephony.CellBroadcastService)
          - GsmCellBroadcastHandler / CdmaCellBroadcastHandler
          - parses the PDU (SmsCbHeader / GsmSmsCbMessage)
          - builds android.telephony.SmsCbMessage
          - deduplicates via CbSendMessageCalculator (serial number)
                                  |
                                  v
        CellBroadcastHandler.broadcastMessage()
          - if message.isEmergencyMessage():
              Intent ACTION_SMS_EMERGENCY_CB_RECEIVED
              intent.putExtra("message", SmsCbMessage)
              intent.setPackage(<default CBR package>)      <-- explicit
              sendOrderedBroadcast(intent, /*permission*/ null, ...)
          - else:
              Intent SMS_CB_RECEIVED_ACTION  (non-emergency path, implicit)
                                  |
                                  v
        packages/apps/CellBroadcastReceiver        [mainline module, privileged]
        CellBroadcastReceiver.onReceive()
          - ACTION_SMS_EMERGENCY_CB_RECEIVED or SMS_CB_RECEIVED
              -> start CellBroadcastAlertService
                                  |
                                  v
        CellBroadcastAlertService
          - handleCellBroadcastIntent(): extract "message" extra
          - shouldDisplayMessage():
               * channel range lookup (CellBroadcastChannelManager)
               * language filter
               * testing_mode gate          <-- see privilege-model.md
               * user toggle gate (CellBroadcastSettings)
          - insert into history database (CellBroadcastContentProvider)
          - openEmergencyAlertNotification(message)
                                  |
                                  +----------------------------+
                                  |                            |
                                  v                            v
        CellBroadcastAlertAudio                     CellBroadcastAlertDialog
          - AudioAttributes USAGE_ALARM                - FLAG_FULLSCREEN
          - ALARM stream, volume forced to full         - FLAG_SHOW_WHEN_LOCKED
          - FLAG_BYPASS_INTERRUPTION_POLICY            - FLAG_TURN_SCREEN_ON
          - FLAG_BYPASS_MUTE                           - FLAG_KEEP_SCREEN_ON
          - Vibrator.vibrate(effect, attrs)            - system-modality full-screen UI
          - alert tone from res/raw/*.ogg              - dismiss button
                                  |                            |
                                  +-------------+--------------+
                                                |
                                                v
                                    Notification (CellBroadcastAlertService
                                    notification channel, emergency category)
                                                |
                                                v
                                            User sees it
```

### A note on the "extra receiver" hooks

Two resource-driven hooks exist that a future experiment should not overlook:

* **CBS → `test_cell_broadcast_receiver_packages`** (`CellBroadcastHandler.broadcastMessage`): when
  `IS_DEBUGGABLE` is true, CBS sends a duplicate of the emergency intent *explicitly* to every package
  in the `com.android.cellbroadcastservice.R.array.test_cell_broadcast_receiver_packages` overlay. The
  AOSP comment says it exists "only for sl4a automation tests". This is a genuine, source-visible
  injection point — but it is *downstream* of CBS, so reaching it requires the message to have come
  from the modem in the first place. It is **not** an alternative to the test app.
* **CBR → `additional_cell_broadcast_receiver_packages`** (CBR's own manifest, plus CBS's send list):
  CBR declares itself as a receiver for a second package name configured by overlay, and CBS sends the
  emergency broadcast to every package in that list. This is how a device can legitimately have two CB
  receivers. It is a configuration mechanism, not a privilege bypass.

Both are recorded here because they are exactly the kind of thing that could be mistaken for a
shortcut. Neither avoids Gate 1.

### Corrections to the naive diagram

The diagram proposed in the project brief was:

```
Cellular modem -> Telephony framework -> CellBroadcastService -> CellBroadcastReceiver -> ...
```

That is **structurally correct but imprecise in two ways** that matter for this project:

1. **`CellBroadcastService` is a separate mainline module, not a class inside telephony.** It lives in
   `packages/modules/CellBroadcastService`, runs in the `com.android.networkstack.process`, and is
   reached only through `ICellBroadcastService`. The telephony framework
   (`CellBroadcastServiceManager`) is a *forwarder*, nothing more.
2. **The receiver is entered by an ordered broadcast, not by a method call.** That distinction is the
   entire security story: the boundary between CBS and CBR is a broadcast with a package target and a
   permission argument.

The brief's "database / alert classification / settings / sound / vibration / full-screen alert" fan-out
is accurate: all of it happens inside `com.android.cellbroadcastreceiver`, driven by
`CellBroadcastAlertService`.

### Where sound, vibration and UI actually come from

Verified in `packages/apps/CellBroadcastReceiver`:

| Effect | Component | File |
| --- | --- | --- |
| Alert sound | `CellBroadcastAlertAudio` (a `Service`) | `src/.../CellBroadcastAlertAudio.java` |
| Vibration | `CellBroadcastAlertAudio`, `Vibrator.vibrate(VibrationEffect, AudioAttributes)` | same |
| Full-screen UI | `CellBroadcastAlertDialog` (an `Activity`) | `src/.../CellBroadcastAlertDialog.java` |
| Notification | `CellBroadcastAlertService` | same |
| History database | `CellBroadcastContentProvider` | same |

Both the audio service and the dialog activity are launched by
`CellBroadcastAlertService.openEmergencyAlertNotification()` via ordinary `startService` /
`startActivity` calls **from inside the privileged CBR app**. They are not public APIs and are not
exported (`CellBroadcastAlertAudio`, `CellBroadcastAlertService` are `android:exported="false"`;
`CellBroadcastAlertDialog` is also `exported="false"`).

Alert classification is resolved in `openEmergencyAlertNotification()`:

* If `message.isEtwsMessage()` -> `SmsCbEtwsInfo` warning type selects the tone
  (earthquake/tsunami/other/test etc.).
* Otherwise -> the channel range's `AlertType` selects the tone.
* Tone resources live in `res/raw/`: `etws_earthquake.ogg`, `etws_tsunami.ogg`,
  `etws_other_disaster.ogg`, `etws_default.ogg`, `default_tone.ogg`, `area.ogg`, `watch_info.ogg`.

## 2. The AOSP test path (verified)

AOSP ships a test application that **enters the pipeline at the receiver, not at the modem**:

```
tests/testapp (package com.android.cellbroadcastreceiver.tests)
    sharedUserId = android.uid.phone
    signed with the platform certificate
        |
        | builds a byte[] PDU (ETWS) or constructs SmsCbMessage directly (CMAS)
        v
    sendOrderedBroadcastAsUser(
        intent = new Intent(ACTION_SMS_EMERGENCY_CB_RECEIVED)   // CMAS path
              or new Intent(SMS_CB_RECEIVED_ACTION),            // generic path
        user   = UserHandle.ALL,
        receiverPermission = RECEIVE_EMERGENCY_BROADCAST / RECEIVE_SMS,
        appOp  = OP_RECEIVE_EMERGECY_SMS / OP_RECEIVE_SMS,
        ...)
        intent.setPackage(<default CBR package>)   // explicit target
        |   intent extra "message" = SmsCbMessage
        v
    CellBroadcastReceiver.onReceive()      <-- enters the REAL receiver
        |
        v
    CellBroadcastAlertService ... (the entire genuine pipeline above)
```

Full detail, including the exact class/method trace and the code quoted verbatim, is in
[`aosp-test-path.md`](aosp-test-path.md). The essential conclusion:

> **The AOSP test path injects at the broadcast boundary between CBS and CBR. Everything downstream —
> emergency classification, channel filtering, database, notification, alarm-stream audio, vibration,
> full-screen UI — is the genuine production code.**

The test path does **not** exercise:

* the modem / RIL,
* the real `CellBroadcastServiceManager` forwarding,
* PDU decoding inside CBS (for the CMAS test path, which builds `SmsCbMessage` directly; the ETWS
  test path *does* go through the test app's own copy of `GsmSmsCbMessage`),
* deduplication as performed by `CbSendMessageCalculator` in CBS.

Note carefully: the test app *does* include its own `GsmSmsCbMessage.java` and `SmsCbHeader` usage —
it decodes the test PDU *itself* and hands the resulting `SmsCbMessage` across the broadcast. So from
CBR's point of view the message looks exactly like a modem-received one.

## 3. The proposed system (NOT implemented)

```
                    +---------------------------------+
                    |          PC CONTROLLER          |
                    |                                 |
                    |  [ SEND TEST ALERT ]            |
                    |  [ CANCEL PENDING ]             |
                    |  alert type / text / targets    |
                    +----------------+----------------+
                                     |
                    local transport (ADB first, Wi-Fi later)
                                     |
              +----------------+-----+-----+----------------+
              |                |           |                |
              v                v           v                v
        Android A          Android B   Android C        Android D
              |                |           |                |
              v                v           v                v
     device-side test bridge (device agent)
              |                |           |                |
              v                v           v                v
     CellBroadcastReceiver.onReceive()  <-- the REAL receiver
              |                |           |                |
              v                v           v                v
     genuine CellBroadcastAlertService / Audio / Dialog
              |                |           |                |
              v                v           v                v
        REAL SYSTEM ALERT: sound + vibration + full-screen UI
```

### The load-bearing constraint

The device-side bridge must be able to *send the same broadcast the AOSP test app sends*. That
requires, at minimum:

1. holding `android.permission.RECEIVE_EMERGENCY_BROADCAST` (protection level `signature|privileged`),
   and
2. running on a build where the broadcast action is permitted from that caller.

Both are privilege questions, answered in [`privilege-model.md`](privilege-model.md). The short
answer is that a stock, unmodified retail device cannot grant (1) to an arbitrary app, so the bridge
cannot be a normal APK. The candidate environments are enumerated in [`feasibility.md`](feasibility.md).

### Separation of concerns (design intent)

The network/control layer and the emergency-alert-processing layer must stay decoupled:

* the controller knows about *devices, messages and acknowledgements*;
* the device agent knows about *one* operation: "inject this test message locally";
* neither layer ever touches the modem, the RIL, or any radio interface.

That separation is what makes the "no cellular transmission" safety property structural rather than a
policy promise. See [`security-and-safety.md`](security-and-safety.md).

## 4. What is deliberately NOT in the architecture

* No modem interaction. No `CbConfig` writes, no channel enabling through the privileged
  `MODIFY_CELL_BROADCASTS` path. Those exist in the codebase and are *not* needed for the test path.
* No fabricated sensor input, no accessibility-service scraping, no overlay drawn on top of the real
  UI.
* No fake notification. If the genuine path proves impossible on a given device, the fallback is
  documented as an explicitly-labelled simulation, not presented as success.
