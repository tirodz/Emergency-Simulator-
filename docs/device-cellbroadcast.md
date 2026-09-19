# Cell Broadcast components on the emulator target

**Experiment:** EXP-ENV-002
**Device:** Android emulator `emulator-5554`, AVD `test35`
**Image:** `system-images;android-35;google_apis;x86_64`
**Status:** CONFIRMED

This document records the Cell Broadcast components actually present on the running target, verified
through `adb` rather than inferred from the AOSP tree.

---

## 1. Build identity

```
ro.build.version.release  = 15
ro.build.version.sdk      = 35
ro.build.type             = userdebug
ro.build.tags             = dev-keys
ro.product.name           = sdk_gphone64_x86_64
ro.product.device         = emu64xa
ro.build.fingerprint      = google/sdk_gphone64_x86_64/emu64xa:15/AE3A.240806.043/12960925:userdebug/dev-keys
ro.debuggable             = 1
```

This is exactly the privilege level the mission calls for: **userdebug with `ro.debuggable=1`**.

## 2. Shell and root identity

```bash
adb shell id
# uid=2000(shell) ... context=u:r:shell:s0

adb root && sleep 5 && adb shell id
# uid=0(root) gid=0(root) ... context=u:r:su:s0
```

Both are available. `adb root` succeeds because the image is userdebug.

## 3. Cell Broadcast packages present

```
package:com.google.android.cellbroadcastreceiver
package:com.google.android.cellbroadcastservice
package:com.android.cellbroadcastreceiver
```

APK/APEX paths, from `pm list packages -f`:

```
/apex/com.android.cellbroadcast/priv-app/GoogleCellBroadcastApp@350820300/GoogleCellBroadcastApp.apk
    = com.google.android.cellbroadcastreceiver

/apex/com.android.cellbroadcast/priv-app/GoogleCellBroadcastServiceModule@350820300/GoogleCellBroadcastServiceModule.apk
    = com.google.android.cellbroadcastservice

/system/priv-app/CellBroadcastLegacyApp/CellBroadcastLegacyApp.apk
    = com.android.cellbroadcastreceiver          (legacy shim on the system partition)
```

**This confirms the earlier offline finding**: Cell Broadcast ships as the
`com.android.cellbroadcast` **APEX**, and the emulator additionally carries a Google build of it
rather than the pure-AOSP package name. The legacy shim coexists on `/system/priv-app`.

Package flags of interest:

```
com.google.android.cellbroadcastservice:
  flags=[ SYSTEM HAS_CODE PERSISTENT ALLOW_CLEAR_USER_DATA ALLOW_BACKUP ]
  codePath=/apex/com.android.cellbroadcast/priv-app/GoogleCellBroadcastServiceModule@350820300

com.google.android.cellbroadcastreceiver:
  privateFlags=[ ... PRIVILEGED ... ]
  pkgFlags=[ SYSTEM HAS_CODE ALLOW_CLEAR_USER_DATA ALLOW_BACKUP ]
  signatures=PackageSignatures{version:3, signatures:[3fe45a]}
```

The service is **PERSISTENT** and the receiver is **PRIVILEGED** — consistent with the privilege model
inverted from the AOSP source.

## 4. The AOSP test application is NOT present

```
adb shell pm list packages | grep -iE 'tests|testapp'      -> (empty)
adb shell find /system /product /vendor /apex -iname '*ellBroadcast*est*'  -> (empty)
adb shell find / -maxdepth 4 -iname '*CellBroadcastReceiverTests*'          -> (empty)
```

**CONFIRMED: the AOSP test app is build-time only and is absent from this image too.** This
reproduces, on a live device, the offline finding from Experiment 3 in `experiments.md`. It is now an
experimental fact rather than an inference.

**Consequence:** Mission 2B's planned "launch `SendTestBroadcastActivity`" cannot be attempted on this
target. There is nothing to launch. This is the first hard confirmation that the "ADB → exported test
Activity" route needs a custom-built APK that this environment cannot produce.

## 5. How a Cell Broadcast actually reaches the receiver

This was traced from AOSP (`android15-release`) to establish the authoritative data contract, because
the test-app shortcut is unavailable and we need to know exactly what the receiver expects.

### 5.1 The delivery mechanism is binder, not an Intent extra

`frameworks/opt/telephony/.../CellBroadcastServiceManager.java`:

```java
Intent intent = new Intent(CellBroadcastService.CELL_BROADCAST_SERVICE_INTERFACE);
intent.setPackage(mCellBroadcastServicePackage);
...
new Intent(CellBroadcastService.CELL_BROADCAST_SERVICE_INTERFACE), ...
```

It binds the service and delivers decoded messages over `ICellBroadcastService` — a binder interface.
There is **no** documented Intent through which an external caller supplies a raw broadcast.

### 5.2 The receiver's dispatch logic

`CellBroadcastReceiver.onReceive()` (858 lines, fetched from `android15-release`), the relevant branch:

```java
} else if (Telephony.Sms.Intents.ACTION_SMS_EMERGENCY_CB_RECEIVED.equals(action) ||
        Telephony.Sms.Intents.SMS_CB_RECEIVED_ACTION.equals(action)) {
    intent.setClass(mContext, CellBroadcastAlertService.class);
    mContext.startService(intent);
}
```

So the receiver **forwards the same Intent** to `CellBroadcastAlertService`. It does not unpack the
message itself.

### 5.3 The alert service's data contract

`CellBroadcastAlertService.java` (1168 lines):

```java
private static final String EXTRA_MESSAGE = "message";
...
private void handleCellBroadcastIntent(Intent intent) {
    Bundle extras = intent.getExtras();
    if (extras == null) {
        Log.e(TAG, "received SMS_CB_RECEIVED_ACTION with no extras!");
        return;
    }
    SmsCbMessage message = (SmsCbMessage) extras.get(EXTRA_MESSAGE);
    if (message == null) {
        Log.e(TAG, "received SMS_CB_RECEIVED_ACTION with no message extra");
        return;
    }
    ...
    if (!shouldDisplayMessage(message)) {
        return;
    }
```

**The expected extra key is the literal string `"message"`, and its value is an
`android.telephony.SmsCbMessage` Parcelable.**

This is the exact injection contract. Note `SmsCbMessage` is a `@SystemApi`/hidden type; constructing
it from an external process requires either the platform SDK stubs or reflection.

### 5.4 The downstream gates

After a message is accepted, `showNewAlert()` applies:

```java
if (channelManager.isEmergencyMessage(cbm) && !sRemindAfterCallFinish) {
    openEmergencyAlertNotification(cbm);     // sound + vibration + full-screen alert
    ...
} else {
    addToNotificationBar(cbm, messageList, this, false, true, false);   // quiet notification
}
```

and `handleCellBroadcastIntent()` additionally requires a matching **enabled channel range**:

```java
if (range != null && range.mDisplay == true) {
    if (provider.insertNewBroadcast(message)) { ... }
```

So even with a perfectly formed message, the alert only becomes an *emergency* experience if the
message's channel is classified as emergency and its range is enabled and marked displayable.

## 6. Binder service surface

```
adb shell cmd -l | grep -iE 'cell|broadcast|telephony'  ->  telephony.registry
adb shell service list | grep -iE 'cellbroadcast'        ->  (none)
adb shell dumpsys cellbroadcast                          ->  Can't find service: cellbroadcast
```

There is **no** `cellbroadcast` / `cmd` shell interface. The CBS module is a persistent app reached
through `CellBroadcastServiceManager`, not a shell-addressable system service. This closes the
"`cmd cellbroadcast ...`" avenue that the earlier report listed as speculative.

## 7. Exported components observed

From `dumpsys package`:

```
Receiver com.google.android.cellbroadcastreceiver/.CellBroadcastReceiver
  Actions: SERVICE_STATE, SMS_SERVICE_CATEGORY_PROGRAM_DATA_RECEIVED,
           CARRIER_CONFIG_CHANGED, DEFAULT_SMS_SUBSCRIPTION_CHANGED,
           SMS_CB_RECEIVED, LOCALE_CHANGED,
           android.provider.action.SMS_EMERGENCY_CB_RECEIVED, BOOT_COMPLETED
  Scheme:  android_secret_code, authority "2627"   (the testing-mode secret code *#*#2627#*#*)

Activity com.google.android.cellbroadcastreceiver/.CellBroadcastListActivity
Activity com.google.android.cellbroadcastreceiver/.CellBroadcastAlertDialog
  Filter: action "android.provider.Telephony.SMS_CB_RECEIVED"
```

## 8. Next action

The test-app route is blocked (no APK, no build capability here). The live question is therefore:

> Can an `SmsCbMessage` be delivered into this pipeline from a root identity on a userdebug build,
> such that Android's own alert service — not a reimplementation — produces the emergency experience?

That is EXP-ALERT-001, recorded in [`experiments.md`](experiments.md).