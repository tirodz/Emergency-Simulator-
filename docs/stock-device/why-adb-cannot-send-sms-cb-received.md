# Why `adb shell am broadcast` to the Cell Broadcast receiver cannot work

Label: **BLOCKED**, with the blocking mechanism **CONFIRMED** against AOSP 14 source.

This note exists because the request "just run `adb shell am broadcast -a
android.provider.Telephony.SMS_CB_RECEIVED -n com.samsung.android.cellbroadcastreceiver/...`" is the
most natural-looking thing in the world and it is the one thing the platform is specifically built
to refuse. Every claim below is quoted from source, so it can be checked rather than believed.

## The never-retried question

> Why does this directory exist, and does it contain an Android project I must finish or wire up?

Answered: `android/alertinject/` holds the single Java source `AlertInjector.java`, built by
`build.sh` into the committed `out/alertinject.jar`. It is the **controlled development path**
(Android 5.0-15, AOSP `CellBroadcastReceiver`, phone/userdebug builds gated on `ro.debuggable=1`,
per `docs/controlled-oem-path.md` and `docs/aosp-test-path.md`). It is a complete project and it is
wired into the controller: `tauri.conf.json` bundles it as `android/alertinject.jar` and
`src-tauri/src/lib.rs` pushes it with `push_injector`/`prepare_test_mode`. Nothing to finish.

`android/local-simulator/` is the **stock-device** path: a Kotlin app that produces a local
notification. It is wired and built by CI.

## Gate 1 - the action is a protected broadcast

`frameworks/base/core/res/AndroidManifest.xml`, `android14-release`, lines 749-750:

```xml
<protected-broadcast android:name="android.provider.Telephony.SMS_CB_RECEIVED" />
<protected-broadcast android:name="android.provider.action.SMS_EMERGENCY_CB_RECEIVED" />
```

## Gate 2 - the shell user is not on the exemption list

`ActivityManagerService.java`, `android14-release`, in `broadcastIntentLocked`, around line 14549:

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

// First line security check before anything else: stop non-system apps from
// sending protected broadcasts.
if (!isCallerSystem) {
    if (isProtectedBroadcast) {
        String msg = "Permission Denial: not allowed to send broadcast "
                + action + " from pid=" + callingPid + ", uid=" + callingUid;
        Slog.w(TAG, msg);
        throw new SecurityException(msg);
```

`SHELL_UID` (2000) is absent from that switch. `adb shell` runs as `SHELL_UID`. Therefore:

* `adb shell am broadcast -a android.provider.Telephony.SMS_CB_RECEIVED` is refused at
  `ActivityManagerService`, before it reaches any receiver, on any device, rooted or not.
* `SHELL_UID` does appear elsewhere in the same file (for example line 15530) as an exemption for
  *other* checks. Its absence *here* is deliberate, not an oversight.
* Rooted stock firmware does not change this: root is not Android, and `isCallerSystem` is computed
  from `callingUid`, which for `am broadcast` is the shell user.

The exact command requested would fail with `SecurityException: Permission Denial: not allowed to
send broadcast android.provider.Telephony.SMS_CB_RECEIVED from pid=..., uid=2000`.

## Gate 3 - the alternative unprotected action leads into a non-exported service

`CellBroadcastAlertService.java` handles a second action, line 89:

```java
public static final String SHOW_NEW_ALERT_ACTION = "cellbroadcastreceiver.SHOW_NEW_ALERT";
```

This action does **not** appear in the platform's protected-broadcast list, so gate 1 does not apply
to it. It is a dead end anyway, because the component that consumes it is not exported
(`packages/apps/CellBroadcastReceiver/AndroidManifest.xml`, lines 77-78):

```xml
<service android:name="com.android.cellbroadcastreceiver.CellBroadcastAlertService"
         android:exported="false" />
```

`exported="false"` means no process outside the CellBroadcast package can start it. An ADB shell
cannot route around that either: the export flag is checked by AMS, not by the shell.

## Gate 4 - Cell Broadcast never travels through Android's networking stack

Worth stating because several workarounds look plausible until this is clear. Cell Broadcast is a
GSM/3GPP service delivered inside the cellular paging and signalling channels (3GPP TS 23.041). It
does not arrive over IP, so there is no socket to connect to, no HTTP endpoint to POST to, and no
Wi-Fi route to insert. The framework reaches its Cell Broadcast stack only through
`RIL` -> `GsmCdmaPhone` -> `IInboundSmsHandler` -> `CellBroadcastHandler`. Every one of those steps
runs in the `phone` process.

## What the two legitimate modes actually are

| Mode | Mechanism | Trigger | Status |
|---|---|---|---|
| Controlled development | AOSP `CellBroadcastReceiver` -> `CellBroadcastAlertService` -> `CellBroadcastAlertDialog` | `ro.debuggable=1` permits the AOSP test entry point that the telephony test code broadcasts | Proven end to end on a `userdebug`/rooted target |
| Stock device | Bundled local simulator app: high-importance notification, full-screen intent, bundled attention tone, vibration | Controller targets the app's own exported receiver by explicit component | Works; it is a local app notification and is **not** a Cell Broadcast |

The stock mode is not a fake of the controlled mode. It is a different, honestly-labelled
instrument: it exercises the operator's *response* (do they see it, hear it, does it take the
screen) rather than the cellular delivery path. The project refuses to blur the two, which is the
entire reason `docs/stock-device/` exists.

## The safety constraint, and why a Shizuku binding does not lift it

Rooting, bootloader unlock, flashing, custom recovery, ROM change, partition remount, privileged
APKs and replacing Android or Samsung components are all excluded by the project's own rules.

A Shizuku binding is not a way around the gates. Shizuku gives an app an `adb`-equivalent binder
handle, which means the caller still presents `SHELL_UID`. Every gate above is evaluated by
`callingUid`, so a Shizuku call hits gate 2 identically. Installing a privileged APK, or replacing
the Samsung CellBroadcast package with a modified build, would change the answer - and is exactly
what the project forbids.

## The conclusion, stated plainly

There is no root-free way to make a retail phone display a genuine Cell Broadcast alert, because
the blocks are in the platform's security model rather than in this code. The honest deliverable is
a working controlled-mode path where the device permits it, plus a clearly-labelled local simulator
for stock hardware, plus diagnostics that say BLOCKED when it is blocked.

If the goal is "an alert that looks and behaves like a real emergency alert on the A35", the two
available routes are:

1. Use a `userdebug`/rooted development device for the controlled path (unchanged, already working).
2. Improve the local simulator's presentation so it is unambiguous on the phone - which is what the
   bundled attention tone, full-screen intent, and lock-screen behaviour changes in this branch are
   for - while still labelling it as a local notification, not a Cell Broadcast.

Claiming otherwise in code or documentation would be the exact conflation this project was built to
avoid.
