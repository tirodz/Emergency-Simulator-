# The AOSP Cell Broadcast test application

This is the single most important document in the investigation. It traces the official AOSP test
application line by line, quotes the decisive code, and states exactly which parts of the emergency
pipeline it exercises.

Source: `packages/apps/CellBroadcastReceiver`, `tests/testapp/`.
Commit inspected: `android16-release` = `b97c8a4ffa3946d7206808bf4810746678b44a5c`
(behaviour verified identical on `android14-release` and `android15-release` for the parts that
matter; differences are called out below).

## 1. Identity of the test application

`tests/testapp/Android.bp`:

```python
android_test {
    name: "CellBroadcastReceiverTests",
    libs: [
        "android.test.runner.stubs.system",
        "telephony-common",
        "android.test.base.stubs.system",
    ],
    srcs: [ "src/**/*.java", ":cellbroadcast-util-shared-srcs" ],
    platform_apis: true,
    // Apk must be signed with platform signature in order to send test broadcasts.
    certificate: "platform",
    instrumentation_for: "CellBroadcastApp",
}
```

`tests/testapp/AndroidManifest.xml`:

```xml
<manifest xmlns:android="http://schemas.android.com/apk/res/android"
    package="com.android.cellbroadcastreceiver.tests"
    android:sharedUserId="android.uid.phone">

    <!-- Test Apk is signed with platform key in order to use this permission. -->
    <uses-permission android:name="android.permission.BROADCAST_SMS"/>
    <uses-permission android:name="android.permission.INTERACT_ACROSS_USERS_FULL"/>
    ...
    <activity android:name="SendTestBroadcastActivity" ... android:exported="true">
        <intent-filter>
            <action android:name="android.intent.action.MAIN" />
            <category android:name="android.intent.category.LAUNCHER" />
        </intent-filter>
    </activity>
</manifest>
```

### Established facts

| Attribute | Value |
| --- | --- |
| Package name | `com.android.cellbroadcastreceiver.tests` |
| Shared UID | `android.uid.phone` (**the phone UID**) |
| Signing certificate | **platform** (the platform signing key) |
| `platform_apis: true` | links against hidden/internal framework APIs |
| Declared permissions | `BROADCAST_SMS` (signature), `INTERACT_ACROSS_USERS_FULL` |
| Build target | `android_test`, part of the AOSP tree |
| Installed by | being built into the platform image (an `android_test` module is not a Play-deliverable app) |

**The APK cannot simply be extracted and installed on a stock retail phone.** Three independent
reasons, any one of which is sufficient:

1. It is signed with the **platform** key. A retail device will not accept a package that claims
   `android.uid.phone` unless it is signed with the platform certificate — and the platform key is
   not public for retail builds.
2. It declares `android:sharedUserId="android.uid.phone"`. `sharedUserId` requires all packages in the
   UID to be signed by the same certificate.
3. It calls into `telephony-common` internals (`com.android.internal.telephony.*`) and is built with
   `platform_apis: true`, so it cannot even be compiled as an ordinary SDK app without AOSP stubs.

## 2. What the test app actually does

### 2.1 The CMAS path — builds `SmsCbMessage` directly and broadcasts it

`tests/testapp/src/com/android/cellbroadcastreceiver/tests/SendGsmCmasMessages.java`:

```java
private static void sendBroadcast(Context context, SmsCbMessage cbMessage) {
    Intent intent = new Intent(Telephony.Sms.Intents.ACTION_SMS_EMERGENCY_CB_RECEIVED);
    intent.putExtra("message", cbMessage);
    intent.setPackage(CellBroadcastUtils.getDefaultCellBroadcastReceiverPackageName(context));
    context.sendOrderedBroadcastAsUser(intent, UserHandle.ALL,
            Manifest.permission.RECEIVE_EMERGENCY_BROADCAST,
            AppOpsManager.OP_RECEIVE_EMERGECY_SMS, null, null, Activity.RESULT_OK, null, null);
}
```

and the message is constructed as:

```java
private static SmsCbMessage createCmasSmsMessage(int serviceCategory, int serialNumber,
        String language, String body, int severity, int urgency, int certainty, int priority) {
    int messageClass = getCmasMessageClass(serviceCategory);
    SmsCbCmasInfo cmasInfo = new SmsCbCmasInfo(
            messageClass,
            SmsCbCmasInfo.CMAS_CATEGORY_UNKNOWN,
            SmsCbCmasInfo.CMAS_RESPONSE_TYPE_UNKNOWN,
            severity, urgency, certainty);
    return new SmsCbMessage(SmsCbMessage.MESSAGE_FORMAT_3GPP, 0, serialNumber,
            new SmsCbLocation("123456"), serviceCategory, language, body,
            priority, null, cmasInfo, 0, 1);
}
```

`SendCdmaCmasMessages.java` uses the same pattern with the CDMA `ACTION_SMS_EMERGENCY_CB_RECEIVED`
broadcast and a CDMA-format `SmsCbMessage`.

### 2.2 The ETWS / generic path — builds a raw PDU and decodes it in the test app

`tests/testapp/src/com/android/cellbroadcastreceiver/tests/SendTestMessages.java`:

```java
private static void sendBroadcast(Context context, int serialNumber, int category,
                                  byte[] pdu) {
    Intent intent = new Intent(Intents.SMS_CB_RECEIVED_ACTION);
    intent.putExtra("message", createFromPdu(context, pdu, serialNumber, category));
    intent.setPackage(CellBroadcastUtils.getDefaultCellBroadcastReceiverPackageName(context));
    context.sendOrderedBroadcastAsUser(intent, UserHandle.ALL, Manifest.permission.RECEIVE_SMS,
            AppOpsManager.OP_RECEIVE_SMS, null, null, Activity.RESULT_OK, null, null);
}
```

`createFromPdu` patches the serial number and message identifier into the hardcoded hex PDU, then
decodes it using the test app's **own copy** of the decoder:

```java
return GsmSmsCbMessage.createSmsCbMessage(context, new SmsCbHeader(pdus[0]),
        sEmptyLocation, pdus, 0 /* slotIndex */);
```

The test app ships `GsmSmsCbMessage.java` (435 lines) and depends on
`com.android.cellbroadcastservice.SmsCbHeader` and `com.android.internal.telephony.gsm.SmsCbConstants`.

Named test entry points (all in `SendTestMessages.java`):

* `testSendEtwsMessageEarthquake` -> `MESSAGE_ID_ETWS_EARTHQUAKE_WARNING`
* `testSendEtwsMessageTsunami` -> `MESSAGE_ID_ETWS_TSUNAMI_WARNING`
* `testSendEtwsMessageEarthquakeTsunami` -> `MESSAGE_ID_ETWS_EARTHQUAKE_AND_TSUNAMI_WARNING`
* `testSendEtwsMessageOther` -> `MESSAGE_ID_ETWS_OTHER_EMERGENCY_TYPE`
* `testSendEtwsMessageTest` -> `MESSAGE_ID_ETWS_TEST_MESSAGE` (`0x1103`)
* `testSendEtwsMessageCancel` -> category `0` (the ETWS cancel PDU)
* plus GSM 7-bit / UCS2 / multipage / language variants of a generic message

### 2.3 The UI

`SendTestBroadcastActivity.java` (702 lines) is a plain `Activity` with one button per test message,
plus fields for message id, category, message body and language code. It sends via the static
helpers above. It is the interactive front end for the same code paths.

## 3. The complete execution trace

```
SendTestBroadcastActivity  (user presses a button)
        |
        v
SendGsmCmasMessages.createCmasSmsMessage(...)      -> new SmsCbMessage(...)
   or  SendTestMessages.createFromPdu(...)         -> GsmSmsCbMessage.createSmsCbMessage(...)
        |
        v
sendBroadcast(context, cbMessage)
        |
        +-- Intent action: ACTION_SMS_EMERGENCY_CB_RECEIVED
        |                  or SMS_CB_RECEIVED_ACTION
        +-- putExtra("message", <SmsCbMessage>)            <-- parcelable payload
        +-- setPackage(<default CBR package>)               <-- explicit target
        |
        v
Context.sendOrderedBroadcastAsUser(
        intent, UserHandle.ALL,
        receiverPermission = RECEIVE_EMERGENCY_BROADCAST / RECEIVE_SMS,
        appOp              = OP_RECEIVE_EMERGECY_SMS     / OP_RECEIVE_SMS,
        ...)
        |
        v
        [ActivityManagerService / BroadcastController]
        |   - protected-broadcast check: is ACTION_SMS_EMERGENCY_CB_RECEIVED protected?
        |     YES -> caller must be a "system" UID (ROOT/SYSTEM/PHONE/NETWORK_STACK/...)
        |   - the test app is in android.uid.phone, so the check passes
        v
com.android.cellbroadcastreceiver.CellBroadcastReceiver.onReceive()
        |   action == ACTION_SMS_EMERGENCY_CB_RECEIVED or SMS_CB_RECEIVED_ACTION
        v
        intent.setClass(mContext, CellBroadcastAlertService.class);
        mContext.startService(intent);
        |
        v
CellBroadcastAlertService.handleCellBroadcastIntent()
        |   SmsCbMessage message = (SmsCbMessage) extras.get("message");
        v
CellBroadcastAlertService.shouldDisplayMessage(message)
        |   - channel range lookup
        |   - language filter
        |   - testing_mode gate for range.mTestMode channels
        |   - user toggle gate
        v
CellBroadcastContentProvider.insertNewBroadcast(message)     <-- REAL history DB
        |
        v
CellBroadcastAlertService.openEmergencyAlertNotification(message)
        |
        +--> CellBroadcastAlertAudio  (startService)
        |        - AudioAttributes USAGE_ALARM / STREAM_ALARM
        |        - FLAG_BYPASS_INTERRUPTION_POLICY | FLAG_BYPASS_MUTE
        |        - ALARM stream volume forced to maximum
        |        - Vibrator.vibrate(effect, audioAttributes)
        |        - TTS of the message body (subject to settings)
        |
        +--> CellBroadcastAlertDialog (startActivity, FLAG_ACTIVITY_NEW_TASK)
        |        - FLAG_FULLSCREEN | FLAG_SHOW_WHEN_LOCKED
        |        - FLAG_TURN_SCREEN_ON | FLAG_KEEP_SCREEN_ON
        |        - dismiss button
        |
        +--> Notification (emergency notification channel)
```

## 4. Which components the test path exercises

| Component | Exercised by test path? |
| --- | --- |
| Broadcast permission check (`RECEIVE_EMERGENCY_BROADCAST` / `RECEIVE_SMS`) | **Yes** |
| AppOp check (`OP_RECEIVE_EMERGECY_SMS` / `OP_RECEIVE_SMS`) | **Yes** |
| Protected-broadcast "is caller system?" check | **Yes** |
| `CellBroadcastReceiver.onReceive` | **Yes** |
| Alert classification / channel lookup | **Yes** |
| `testing_mode` gate | **Yes** |
| User settings toggles | **Yes** |
| History database (`CellBroadcastContentProvider`) | **Yes** |
| Notification | **Yes** |
| Alert sound (`CellBroadcastAlertAudio`) | **Yes** |
| Vibration | **Yes** |
| Full-screen UI (`CellBroadcastAlertDialog`) | **Yes** |
| Screen wake / lock-screen display | **Yes** |
| DND bypass behaviour | **Yes** (subject to channel `override_dnd` / settings) |
| Accessibility / TTS | **Yes** (subject to settings) |
| Dismissal | **Yes** (the dialog's own dismiss) |
| Modem / RIL | **No** |
| `CellBroadcastServiceManager` forwarding | **No** |
| CBS PDU decoding + dedup (`CbSendMessageCalculator`) | **No** (CMAS path). Partially bypassed on the ETWS path, where the test app decodes the PDU itself |
| Duplicate-suppression *across* CBS and CBR | **No** — CBR dedups on the history DB, CBS dedups on the modem path |

### The exact answer to the brief's Path A / Path B question

> **Path B enters the system at the ordered-broadcast boundary that Path A also crosses.** Everything
> after that boundary — which is where all the *user-visible* emergency behaviour lives — is the same
> production code. Everything *before* that boundary (radio, modem, RIL, framework forwarding, real
> PDU decode, network-level dedup) is simulated or skipped.

This is precisely the property the project wants: the expensive-to-fake part (sound, vibration,
full-screen UI, DND override, database) is genuine.

## 5. Build-type and OEM dependencies of the test app

| Dependency | Detail |
| --- | --- |
| Platform certificate | Required. Not reproducible on a retail device. |
| `android.uid.phone` shared UID | Required for the protected-broadcast caller check. |
| `platform_apis: true` | Required to compile against `com.android.internal.telephony.*`. |
| Presence in the image | The module is `android_test`; it is built with the platform, not downloaded. |
| Testing mode | Some *channels* additionally require CBR's testing mode; see §6. |
| OEM | The test app itself is AOSP. OEM variants of CBR may differ in channel configuration and in `allow_testing_mode_on_user_build`; see [`oem-compatibility.md`](oem-compatibility.md). |

## 6. The testing-mode switch (independent of the test app)

`packages/apps/CellBroadcastReceiver/src/.../CellBroadcastReceiver.java`, `onReceive`:

```java
} else if (TelephonyManager.ACTION_SECRET_CODE.equals(action)) {
    if (SystemProperties.getInt("ro.debuggable", 0) == 1
            || res.getBoolean(R.bool.allow_testing_mode_on_user_build)) {
        setTestingMode(!isTestingMode(mContext));
        ...Toast "testing mode enabled/disabled"...
    }
}
```

* Entered via the dialer secret code **`*#*#2627#*#*`**, declared in the app manifest:
  ```xml
  <intent-filter>
      <action android:name="android.telephony.action.SECRET_CODE" />
      <!-- CMAS: To toggle test mode for cell broadcast testing on userdebug build -->
      <data android:scheme="android_secret_code" android:host="2627" />
  </intent-filter>
  ```
* Enabled when `ro.debuggable == 1` (userdebug/eng) **or** when the resource
  `allow_testing_mode_on_user_build` is `true`.
* In AOSP `res/values/config.xml` that resource is `true`. The MCC/MNC overlay
  `res/values-mcc440-mnc20/config.xml` (Japan, NTT docomo) sets it to `false`.
* Once enabled, CBR writes `testing_mode=true` into its default `SharedPreferences`, which:
  * allows channels marked `testing_mode=true` to pass the `shouldDisplayMessage` gate, and
  * makes several test toggles visible in settings (`isExerciseTestAlertsToggleVisible`,
    `isOperatorTestAlertsToggleVisible`, `isTestAlertsToggleVisible`).

**Note:** testing mode is *not* what makes the broadcast legal. It only affects which *channels* are
accepted after the broadcast has already passed the security check. A future experiment should confirm
this separation (see `experiments.md`).

## 7. What the test app cannot do

* It cannot be installed on an unmodified retail phone (signing + shared UID + internal APIs).
* It cannot make a channel marked `debug_build=true` appear unless `ro.debuggable == 1`.
* It cannot enable a channel whose user toggle is off (`shouldDisplayMessage` still applies).
* It cannot transmit anything over the air. It never touches the modem.
* It cannot reproduce the CBS-side dedup or the modem-side reception semantics.

## 8. Consequences for our design

1. The "golden path" is real: **the AOSP test mechanism does drive the genuine emergency UI, sound and
   vibration.** This is the strongest confirmed result of the investigation.
2. The golden path is **not available to an ordinary APK**. It requires platform signing *and* a
   system UID.
3. Therefore the project needs either (a) a device we are allowed to reflash with an AOSP
   userdebug/eng build, (b) a rooted device where we can install a system app, or (c) a different,
   still-to-be-verified injection point. Option (c) is the subject of the open questions.

## 9. Corrections and additions from image analysis (Experiment 3)

Reading a real Google AOSP image byte-for-byte (`docs/experiments.md`, Experiment 3) corrected or
sharpened several points that had been inferred from source browsing alone.

### 9.1 The emergency action string

The constant `Telephony.Sms.Intents.ACTION_SMS_EMERGENCY_CB_RECEIVED` expands to:

```
android.provider.action.SMS_EMERGENCY_CB_RECEIVED
```

Note `provider.action`, **not** `provider.Telephony`. This is easy to get wrong and a wrong action
string produces a silent no-op rather than an error. The non-emergency action is
`android.provider.Telephony.SMS_CB_RECEIVED`. Both were read out of the shipping
`framework-res.apk`'s `<protected-broadcast>` list:

```
protected-broadcast    android.provider.Telephony.SMS_CB_RECEIVED
protected-broadcast    android.provider.action.SMS_EMERGENCY_CB_RECEIVED
protected-broadcast    com.android.cellbroadcastreceiver.GET_LATEST_CB_AREA_INFO
```

This is empirical confirmation that the `BroadcastController.isCallerSystem` gate applies to both
Cell Broadcast entry actions on a shipping **user** build, not merely in the source tree.

### 9.2 The test APK ships on no build type at all

`CellBroadcastReceiverTests` is not present anywhere in the AOSP GSI. A recursive walk of the whole
image found only the APEX module and a separate `CellBroadcastLegacyApp` shim:

```
/system/apex/com.android.cellbroadcast.capex
/system/priv-app/CellBroadcastLegacyApp/...
```

No `test` entries exist inside the APEX payload either. Combined with the absence of the target from
any `PRODUCT_PACKAGES`, this means the test app is **build-time only**: it is not installed by
default on user, userdebug or eng. It must be built and pushed explicitly, and the build must be
platform-signed.

**Consequence for the milestone:** there is no "ADB-only" path that works on a stock or
already-flashed device. `adb shell am start` needs the APK to exist on the device first, which
requires either a custom build or a rooted device where a platform-signed APK can be placed in
`/system/priv-app` (or installed as a system app).

### 9.3 The receiver's real permission set

The privileged allowlist shipped in the APEX is:

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

`RECEIVE_EMERGENCY_BROADCAST` is granted to the receiver only through this allowlist.
`START_ACTIVITIES_FROM_BACKGROUND` is what allows the alert dialog to appear over the current app.

### 9.4 Where the components live

The receiver and service ship inside the Cell Broadcast APEX
(`/system/apex/com.android.cellbroadcast.capex`, manifest name `com.android.cellbroadcast`), as
`CellBroadcastApp@<build>/CellBroadcastApp.apk` and `CellBroadcastServiceModule@<build>/`. They are
no longer plain `system/priv-app` entries. This confirms the Mainline/APEX architecture claim.

### 9.5 The activity cannot be driven by an Intent

`SendTestBroadcastActivity.onCreate()` only calls `setContentView(R.layout.test_buttons)` and wires
`OnClickListener`s. There is no `onNewIntent` override and no `getIntent()` call anywhere in the
class (verified by grep across all 702 lines). **Therefore no Intent extra can trigger a test
message.** The activity is a pure GUI.

Consequence: `adb shell am start` will open the UI and *nothing else*. To actually send, a subsequent
input step is required — either `adb shell input tap x y` on a button, or UI automation via
`uiautomator`. Button resource IDs are stable and known:

| Test category | Resource ID |
| --- | --- |
| ETWS test | `button_etws_test_type` |
| ETWS earthquake | `button_etws_earthquake_type` |
| ETWS tsunami | `button_etws_tsunami_type` |
| ETWS other | `button_etws_other_type` |
| ETWS cancel | `button_etws_cancel_type` |
| GSM CMAS monthly test | `button_gsm_cmas_monthly_test` |
| GSM CMAS exercise test | `button_gsm_cmas_exercise_test` |
| GSM state/local test | `button_gsm_state_local_test_alert` |
| GSM public safety | `button_gsm_public_safety_message` |
| CDMA CMAS monthly test | `button_cmas_monthly_test` |

Screen coordinates for these must be read at run time from `uiautomator dump`, since they depend on
resolution and layout. Do not hardcode coordinates.

### 9.6 Correction to the "test app is an instrumented test" assumption

`tests/testapp/src/` contains **no JUnit test classes at all** — no `extends TestCase`, no
`@Test`, no instrumentation test bodies. The `instrumentation` element in its manifest targets
`com.android.cellbroadcastreceiver`, but there is nothing for the runner to execute. Running
`am instrument` against the testapp package would find no tests.

This means **`am instrument` is not a usable non-UI trigger path** for the CMAS/ETWS buttons. The
only way to invoke the send code is through the activity's UI callbacks. That is a significant
constraint on automating the proof of concept: the controller must drive the GUI, not a test hook.

The separate `tests/unit/` module (`CellBroadcastReceiverUnitTests`,
`CellBroadcastReceiverPlatformUnitTests`, `certificate: "platform"` / `"networkstack"`) does contain
real tests, but they are unit tests of the receiver's logic; they do not exercise the production
alert UI on a device and are not a substitute for the end-to-end proof.

### 9.7 What this means for the next milestone

The chain the milestone hoped to prove is:

```
ADB → exported test Activity → test code → real CellBroadcastReceiver → genuine alert
```

It remains a **valid** chain, but with two hard preconditions now confirmed:

1. The test APK must exist on the device. It ships nowhere, so this requires a custom AOSP build or a
   rooted device.
2. ADB cannot complete the chain alone — after launching the activity, it must inject a UI tap to
   invoke the send callback, because the activity has no Intent-driven path.

The device-side prerequisites for the minimal proof of concept are therefore:

```
device        : userdebug/eng AOSP build or a rooted device
+build target : CellBroadcastReceiverTests   (m CellBroadcastReceiverTests)
+artifact      : out/target/product/<device>/testcases/CellBroadcastReceiverTests/...apk
+install      : platform-signed; push into /system/priv-app (or install as system app)
+drive        : am start  +  input tap (or uiautomator)
+observe      : logcat + Android's own Cell Broadcast history
```

Note the build target is the Soong module name `CellBroadcastReceiverTests`, not the Java package
name. Its `instrumentation_for` is `CellBroadcastApp`.

## 10. Correction — the test app is not required, and it is not the only trigger

This section supersedes the earlier conclusion that the AOSP test application is the necessary
gateway. Two things are now known from the device work in
[`experiments.md`](experiments.md).

### 10.1 The receiver's own entry point is the mechanism

`CellBroadcastReceiver` accepts its input as a `SmsCbMessage` Parcelable under the `"message"` extra
of the protected broadcast `android.provider.action.SMS_EMERGENCY_CB_RECEIVED`. Any caller that can
construct that object and pass the `<protected-broadcast>` UID check reaches the identical pipeline
that the test app reaches. The test app is a convenience built on top of this; it is not a
prerequisite.

Confirmed by execution: a locally built tool running as root produced the real alert dialog, real
audio, real TTS and a real history-database row. See EXP-ALERT-002.

The practical consequence is that **building `CellBroadcastReceiverTests` is off the critical path**
and should not be attempted. It remains useful only as documentation of the officially supported
channel choices.

### 10.2 Testing mode has a vendor-supported trigger, and it is not dialler-only

§6 above records testing mode being toggled by the dialler secret code `*#*#2627#*#*`. That code
resolves to a broadcast, and the broadcast can be sent directly — which is what a controller would
do, since it cannot type into the dialler reliably:

```
adb shell am broadcast -a android.telephony.action.SECRET_CODE \
    -d "android_secret_code://2627"
```

Verified on the device:

```
D CellBroadcastReceiver: onReceive Intent { act=android.telephony.action.SECRET_CODE
    dat=android_secret_code://2627 cmp=.../CellBroadcastReceiver }
D CellBroadcastReceiver: Cell broadcast testing mode is disabled.
D CellBroadcastReceiver: Cell broadcast testing mode is enabled.
```

It is a **toggle**, not a setter — sending it twice returns to the original state. A controller must
therefore read the resulting state rather than assume the send turned testing mode on.

This is the cleaner way to reach testing mode than editing the shared-prefs file, and it is the
vendor's own mechanism rather than a workaround. Note, however, that it only flips `testing_mode`;
`enable_test_alerts` is a separate user toggle and still has to be set. On a device whose settings UI
does not expose that toggle, editing the preference file remains the fallback.

### 10.3 What the test app would still tell us

One thing our injector does not do is choose the *production* channel and warning-type combinations
that AOSP's test app offers as buttons. Those combinations are documented in §1 and are worth
reusing as our controller's alert-type menu, so that every preset we offer is one AOSP itself
identified as safe to send in a test.