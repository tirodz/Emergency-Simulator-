# The Cell Broadcast gate model, from primary source

Every claim here was read out of AOSP source fetched during this session, not paraphrased from a
prior note. Where a claim is about the **Samsung A35 specifically**, it is labelled `UNKNOWN`,
because that device has not yet produced evidence.

The point of this document is that "can this device show a test emergency alert" is not one gate. It
is **five**, and four of them are invisible from the outside: the broadcast is accepted, `am` prints
success, and nothing happens. Each gate is recorded with where it lives and what evidence would
settle it on a real device.

---

## The two legitimate paths

There are exactly two software paths in AOSP by which a test harness can reach the real Cell
Broadcast pipeline. They have **different gates**, and conflating them is the error to avoid.

### Path 1 — the telephony test receiver (no app needed)

`frameworks/opt/telephony`, `GsmInboundSmsHandler`:

```java
private static final boolean TEST_MODE = SystemProperties.getInt("ro.debuggable", 0) == 1;
private static final String TEST_ACTION = "com.android.internal.telephony.gsm"
        + ".TEST_TRIGGER_CELL_BROADCAST";

if (TEST_MODE) {
    mTestBroadcastReceiver = new GsmCbTestBroadcastReceiver();
    IntentFilter filter = new IntentFilter();
    filter.addAction(TEST_ACTION);
    context.registerReceiver(mTestBroadcastReceiver, filter, Context.RECEIVER_EXPORTED);
}
```

* Registered **dynamically**, so it has no manifest component name. `am broadcast -n` cannot target
  it; the action alone must be used.
* Registered `RECEIVER_EXPORTED` with **no permission**, so any UID may send to it — including the
  ordinary adb shell.
* Entered with `--es pdu_string <hex>`; `--es pdu <bytearray>` is the alternative extra.
* The PDU goes to `mCellBroadcastServiceManager.sendGsmMessageToHandler(...)`, which places it on
  `EVENT_NEW_GSM_SMS_CB` — the **same handler path a real radio notification uses**.

**Gate 1: `ro.debuggable == 1`**, read once at class initialisation.

### Path 2 — the CellBroadcast app's testing mode (no debug build needed)

`packages/apps/CellBroadcastReceiver`. A dialer secret code toggles a preference:

```xml
<action android:name="android.telephony.action.SECRET_CODE" />
<data android:scheme="android_secret_code" android:host="2627" />
```

so the code is `*#*#2627#*#*`. The handler:

```java
} else if (TelephonyManager.ACTION_SECRET_CODE.equals(action)) {
    if (SystemProperties.getInt("ro.debuggable", 0) == 1
            || res.getBoolean(R.bool.allow_testing_mode_on_user_build)) {
        setTestingMode(!isTestingMode(mContext));
```

**Gate 2: `ro.debuggable == 1` **or** the app resource `allow_testing_mode_on_user_build`.** That
resource defaults to **`true`** in AOSP `res/values/config.xml`.

This is the path that does **not** require a debug build, and it is the one most likely to be
overlooked. It is also the one an OEM can remove by setting the resource to `false`, by stripping the
secret-code intent filter, or by replacing the app.

---

## The three suppression gates, after the message enters the pipeline

These sit *downstream* of both paths, so a message that passes Gate 1 or Gate 2 can still vanish.

### Gate 3 — OEM master switch (kills every CB message including tests)

`CellBroadcastServiceManager.sendGsmMessageToHandler` runs **before** the message reaches the
handler:

```java
public void sendGsmMessageToHandler(Message m) {
    if (cbMessagesDisabledByOem()) {
        Log.d(TAG, "GSM CB message ignored - CB messages disabled by OEM.");
        CellBroadcastStatsLog.write(... CELL_BROADCAST_MESSAGE_FILTERED__TYPE__GSM,
                ... FILTER__DISABLED_BY_OEM);
        return;
    }
```

and

```java
private boolean cbMessagesDisabledByOem() {
    return mContext.getResources().getBoolean(
            com.android.internal.R.bool.config_disable_all_cb_messages);
}
```

This is a **framework resource boolean**, overridable by an RRO overlay. An OEM that sets it to
`true` disables the entire feature, and the log line is the only trace. It is invisible from
`pm list packages` and from `ro.debuggable`.

### Gate 4 — test-mode-only channel ranges

`CellBroadcastAlertService`:

```java
// If the alert is set for test-mode only, then we should check if device is currently under
// testing mode (testing mode can be enabled by dialer code *#*#CMAS#*#*.
if (range != null && range.mTestMode && !CellBroadcastReceiver.isTestingMode(mContext)) {
    Log.d(TAG, "ignoring the alert due to not in testing mode");
    CellBroadcastReceiverMetrics.getInstance()
            .logMessageFiltered(FILTER_NOTSHOW_TESTMODE, message);
    return false;
}
```

`mTestMode` is a property of the **channel range**, not of the message identifier. So whether an ETWS
test message (0x1103) is displayed depends on how the carrier configuration defines its channel's
range — not on the fact that it is a test. This is why testing mode can be required even for a
message whose ID says "test".

### Gate 5 — the channel must be enabled

A message on a channel with no enabled range is discarded silently. `dumpsys carrier_config` shows
whether a subscription restricts CB, and `settings global cell_broadcast_*` shows the user state.
Documented here because a correct PDU on a disabled channel produces exactly the same visible result
as a broken injection — nothing.

---

## Why a normal app gets `SecurityException`, but adb shell does not

This is the question the task brief asks, and the answer is precise.

`android.provider.Telephony.SMS_CB_RECEIVED` and `android.provider.action.SMS_EMERGENCY_CB_RECEIVED`
are both in the platform's protected-broadcast list (`frameworks/base/core/res/AndroidManifest.xml`):

```xml
<protected-broadcast android:name="android.provider.Telephony.SMS_CB_RECEIVED" />
<protected-broadcast android:name="android.provider.action.SMS_EMERGENCY_CB_RECEIVED" />
```

The check that throws is in `ActivityManagerService` / `BroadcastController`
(`services/core/java/com/android/server/am/`):

```java
final boolean isCallerSystem;
switch (UserHandle.getAppId(callingUid)) {
    case ROOT_UID:
    case SYSTEM_UID:
    case PHONE_UID:
    case BLUETOOTH_UID:
    case NFC_UID:
    case SE_UID:
    case NETWORK_STACK_UID:
        isCallerSystem = true;
        break;
    default:
        isCallerSystem = (callerApp != null) && callerApp.isPersistent();
        break;
}

if (!isCallerSystem) {
    if (isProtectedBroadcast) {
        throw new SecurityException(msg);   // "not allowed to send broadcast"
    }
```

**`SHELL_UID` is not in that list.** Against `android.os.Process`: `ROOT_UID = 0`,
`SYSTEM_UID = 1000`, `PHONE_UID = 1001`, `SHELL_UID = 2000`.

Verified on `main`, `android13-release` and `android14-release` — the same seven UIDs, with
`SHELL_UID` absent on all three.

So an ordinary app targeting `SMS_CB_RECEIVED` is refused by this check, because it is neither a
system UID nor a persistent process. **That is the SecurityException, and it is a protected-broadcast
check, not a permission check.** `pm grant` cannot fix it; there is no permission to grant.

But adb shell does not need to send that intent at all, because **`TEST_TRIGGER_CELL_BROADCAST` is
not a protected broadcast** — it does not appear in the platform manifest's protected list. It is an
ordinary exported receiver, which is why `uid=2000` may send it. The broadcast is sent by
`ActivityManagerShellCommand`, which adds `FLAG_RECEIVER_FROM_SHELL`; that flag only suppresses a
warning log in `checkBroadcastFromSystem`, so it is not the reason the send succeeds.

The reason adb shell works is narrower and more useful than "adb has privileges": **the shell can
send the test action, and only that action.** "ADB is transport, not privilege escalation" — the
command being accepted is not Android authorising a Cell Broadcast.

---

## Shizuku: why it is not the answer

Shizuku hands an app a Binder proxy that runs calls as `shell` (uid 2000). That is the same UID adb
already gives us, so:

* It cannot send a protected broadcast — uid 2000 is not in the exempt list above.
* It cannot change `ro.debuggable`, which is read at class initialisation.
* It cannot make a dynamically-registered receiver exist when `TEST_MODE` is false.
* It cannot change a framework resource (`config_disable_all_cb_messages`).

Shizuku would add an install step and a user-granted privilege for **strictly no additional reach**.
It is documented here as a non-solution so it is not re-proposed.

---

## Summary table

| # | Gate | Where | Decides | Settled by |
| --- | --- | --- | --- | --- |
| 1 | `ro.debuggable == 1` | `GsmInboundSmsHandler` class init | Path 1 exists at all | `getprop ro.debuggable` |
| 2 | `ro.debuggable == 1` OR `allow_testing_mode_on_user_build` | `CellBroadcastReceiver` secret-code branch | Path 2 can be enabled | app resource + `ro.debuggable` |
| 3 | `config_disable_all_cb_messages` | `CellBroadcastServiceManager` | *every* CB message dropped | framework resource |
| 4 | channel range `mTestMode` vs device testing mode | `CellBroadcastAlertService` | test alert filtered | carrier config + testing mode |
| 5 | channel enabled / carrier restriction | carrier config, settings | alert discarded | `dumpsys carrier_config`, settings |

| Claim | Label |
| --- | --- |
| Paths 1 and 2 exist and have the stated gates in AOSP | `CONFIRMED` (source read this session) |
| `SHELL_UID` is not exempt from the protected-broadcast check | `CONFIRMED` (three branches) |
| `TEST_TRIGGER_CELL_BROADCAST` is not a protected broadcast | `CONFIRMED` |
| `allow_testing_mode_on_user_build` defaults to `true` in AOSP | `CONFIRMED` |
| Which of these gates the A35 actually has | `UNKNOWN` — no evidence yet |
| Samsung has not modified any of the above | `UNKNOWN` — must not be assumed in either direction |

---

## Log markers: what is actually emitted, and what it is worth

Every marker below was read out of the AOSP sources this session. The distinction between a
*positive* marker (the pipeline ran) and a *negative* marker (the platform dropped the message and
said why) matters, because a partial capture can contain both.

### Negative markers — the platform states a cause

| Log line (verbatim) | Gate | Emitted by |
| --- | --- | --- |
| `GSM CB message ignored - CB messages disabled by OEM.` | 3 | `CellBroadcastServiceManager` |
| `CDMA CB message ignored - CB messages disabled by OEM.` | 3 | `CellBroadcastServiceManager` |
| `CDMA SCP CB message ignored - CB messages disabled by OEM.` | 3 | `CellBroadcastServiceManager` |
| `ignoring the alert due to not in testing mode` | 4 | `CellBroadcastAlertService` |
| `ignoring the alert due to configured channels was marked ...` | 5 | `CellBroadcastAlertService` |
| `ignoring the alert due to language mismatch. Message lang=` | 5 | `CellBroadcastAlertService` |
| `Skipped message due to filter: ` | 5 | `CellBroadcastAlertService` |

These are the most trustworthy lines in a capture: each is emitted only after the message reached
the platform and was deliberately discarded, and each names its own cause. A run that produces one is
a **definite negative result**, not an unknown, and re-running cannot change it.

### Positive markers — the pipeline ran

| Log line (verbatim) | Stage claimed |
| --- | --- |
| `GsmInboundSmsHandler: Received test intent action=` | `TEST ENTRY POINT ACCEPTED` |
| `CBAlertService: onStartCommand` | `ALERT SERVICE REACHED` |
| `CellBroadcastReceiver: onReceive Intent { act=android.provider.Telephony.SMS_CB_RECEIVED` | `CB RECEIVER PROCESSED` |
| `CellBroadcastReceiver: onReceive Intent { act=android.provider.action.SMS_EMERGENCY_CB_RECEIVED` | `CB RECEIVER PROCESSED` |
| `openEmergencyAlertNotification` | `NATIVE ALERT PRESENTED` |

### Two traps in these markers, both of which we fell into

**The receiver tag fires on actions the app rejects.** `CellBroadcastReceiver.onReceive` begins with
`if (DBG) log("onReceive " + intent)`, and `DBG` is hardcoded `true`. It runs for *every* action
handed to the receiver, including the `onReceive() unexpected action` fallback. So the bare tag
`CellBroadcastReceiver` proves only that the process exists — not that it processed anything. The
marker must be the receiver's own intent dump carrying a Cell-Broadcast action, which is the only
form that appears when that receiver was entered with that action.

**`CBAlertService: onStartCommand` fires before the message is judged.** It is emitted at the top of
`onStartCommand`, before the testing-mode, channel-range and language checks run. A gated message
therefore produces *both* this line and a suppression line. The suppression is the more specific and
more final fact, so a verdict must consult it first; checking "did anything positive happen" first
would report a dropped message as delivered.

### Stage ordering

`ReceiverProcessed` is a distinct stage between `CellBroadcastServiceReached` and `AlertServiceReached`.
The receiver running re-dispatches into the alert service, but it is still before the alert service
has looked at the message, so it must not be reported as `NATIVE ALERT PRESENTED`.

There is a second, finer distinction on the other side of the same line. `AlertServiceReached` is its
own stage because `CBAlertService: onStartCommand` fires *before* the service's channel-range and
testing-mode gates run. Only `openEmergencyAlertNotification`, which the service emits after it has
decided to present, reaches the top stage. The two were once one marker set, and that mapping would
have reported a suppressed message as a presented alert; they were split this session and
`PlatformEvidence::verification_stage()` walks the same ladder, with `SUCCESS` unreachable for a
suppressed capture.

| Claim | Label |
| --- | --- |
| The marker strings above are the exact AOSP log text | `CONFIRMED` (source read this session) |
| The receiver tag is not by itself evidence of processing | `CONFIRMED` |
| Which markers the A35's Samsung firmware emits verbatim | `UNKNOWN` — no capture yet |
