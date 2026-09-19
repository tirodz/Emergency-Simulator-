# Privilege model and security boundary

This document answers, with cited AOSP source, precisely who can trigger Android's genuine emergency
alert pipeline. It is the basis for every "yes/no/requires root" answer elsewhere in the repository.

Sources cited:

* `frameworks/base/+/refs/heads/main/core/res/AndroidManifest.xml` (permission protection levels,
  protected broadcasts)
* `frameworks/base/+/refs/heads/main/services/core/java/com/android/server/am/BroadcastController.java`
* `frameworks/base/+/refs/heads/main/core/java/android/provider/Telephony.java`
* `packages/apps/CellBroadcastReceiver` (`android16-release` = `b97c8a4ffa3946d7206808bf4810746678b44a5c`)
* `packages/modules/CellBroadcastService`

> **Verified against a real image.** The claims in this document were checked byte-for-byte
> against a Google-published AOSP system image (Android 17 / SDK 37, `user` build,
> SHA-256 `9aa638ec20577ac4d15610527d2da2e7e3fc8388ae7ca23c2de3cb4e3df535c1`). See `docs/experiments.md`, Experiment 3. In particular the
> `<protected-broadcast>` declarations and the receiver's privileged-permission allowlist
> were read out of the shipping image rather than inferred from documentation.


## 1. The four gates

To trigger the genuine alert through the AOSP test mechanism, a caller must pass **four independent
gates**. Failing any one is fatal.

### Gate 1 — Protected broadcast: the caller must be a system UID

`core/res/AndroidManifest.xml` declares:

```xml
<protected-broadcast android:name="android.provider.Telephony.SMS_CB_RECEIVED" />
<protected-broadcast android:name="android.provider.action.SMS_EMERGENCY_CB_RECEIVED" />
```

`services/core/java/com/android/server/am/BroadcastController.java`:

```java
// Verify that protected broadcasts are only being sent by system code,
// and that system code is only sending protected broadcasts.
final boolean isProtectedBroadcast;
isProtectedBroadcast = AppGlobals.getPackageManager().isProtectedBroadcast(action);
...
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
        String msg = "Permission Denial: not allowed to send broadcast " + action
                + " from pid=" + callingPid + ", uid=" + callingUid;
        Slog.w(TAG, msg);
        throw new SecurityException(msg);
    }
    ...
}
```

**Conclusion (CONFIRMED):** an ordinary third-party APK cannot send this broadcast, *regardless of
what permissions it holds*. The check is on the UID, and it runs before any permission check. This is
the correct reading of the "protected broadcast" mechanism and it is the primary reason the test app
uses `android:sharedUserId="android.uid.phone"`.

Accepted system UIDs: `ROOT_UID`, `SYSTEM_UID`, `PHONE_UID`, `BLUETOOTH_UID`, `NFC_UID`, `SE_UID`,
`NETWORK_STACK_UID`, or a persistent app.

This list explains the design of both mainline modules exactly:

| Component | Identity | Passes Gate 1? |
| --- | --- | --- |
| CBS (`com.android.cellbroadcastservice`) | `sharedUserId="android.uid.networkstack"` → `NETWORK_STACK_UID` | **Yes, by identity** |
| CBR (`com.android.cellbroadcastreceiver[.module]`) | no `sharedUserId`; only `privileged: true` + platform-class certificate | **No** — it only ever *receives*, never sends |

So CBS is a system UID on purpose: it must be able to send the protected broadcast. CBR deliberately
is not, because it has no need to send one.

**Resolved sub-question:** the protected-broadcast check does *not* skip the permission question. The
separate `BroadcastSkipPolicy` checks (Gate 2 below) enforce `receiverPermission` for the sender and
for the receiving manifest receiver. So a system UID is necessary but the permission is also checked.
(Confirmed from source; still worth one confirming experiment — `experiments.md`, Experiment 7.)

### Gate 2 — The explicit package target

The test app calls `intent.setPackage(...)` with the default CBR package. Only the real
`CellBroadcastReceiver` handles the action, and its `onReceive` dispatches unconditionally to
`CellBroadcastAlertService`:

```java
} else if (Telephony.Sms.Intents.ACTION_SMS_EMERGENCY_CB_RECEIVED.equals(action) ||
        Telephony.Sms.Intents.SMS_CB_RECEIVED_ACTION.equals(action)) {
    intent.setClass(mContext, CellBroadcastAlertService.class);
    mContext.startService(intent);
}
```

There is **no caller-identity check inside the receiver itself** — the app trusts the platform's
broadcast enforcement entirely. That is why Gate 1 is the load-bearing gate.

**However**, the platform *does* check the *receiver* side too. Verified in
`services/core/java/com/android/server/am/BroadcastSkipPolicy.java`:

```java
if (info.activityInfo.applicationInfo.uid != Process.SYSTEM_UID &&
        r.requiredPermissions != null && r.requiredPermissions.length > 0) {
    for (int i = 0; i < r.requiredPermissions.length; i++) {
        final int perm = hasPermissionForDataDelivery(...) ? GRANTED : DENIED;
        if (perm != PackageManager.PERMISSION_GRANTED) {
            return "Permission Denial: receiving " + r.intent + " to " + ... +
                    " requires " + requiredPermission + ...;
        }
    }
}
if (r.appOp != AppOpsManager.OP_NONE) {
    if (!noteOpForManifestReceiver(r.appOp, r, info, component)) {
        return "Skipping delivery to " + ... + " due to required appop " + r.appOp;
    }
}
```

and for registered (runtime) receivers, on the *sender* side:

```java
// Check that the sender has permission to send to this receiver
if (filter.requiredPermission != null) {
    int perm = checkComponentPermission(filter.requiredPermission, r.callingPid, r.callingUid, -1, true);
    if (perm != PackageManager.PERMISSION_GRANTED) {
        return "Permission Denial: broadcasting " + r.intent + " ... requires " + filter.requiredPermission;
    } else {
        final int opCode = AppOpsManager.permissionToOpCode(filter.requiredPermission);
        if (opCode != AppOpsManager.OP_NONE &&
                mService.getAppOpsManager().noteOpNoThrow(opCode, r.callingUid, ...) != MODE_ALLOWED) {
            return "Appop Denial: broadcasting ... requires appop ...";
        }
    }
}
```

**Consequences:**

* The `receiverPermission` argument passed by the test app is enforced on **both** sides: the sender
  must hold it, and the receiving manifest receiver must hold it (the receiver-side check is skipped
  only when the receiver is `SYSTEM_UID`).
* CBR is *not* `SYSTEM_UID` (it runs as `networkstack_uid`/its own uid depending on variant), so it is
  checked — and it passes because its privapp allowlist grants `RECEIVE_EMERGENCY_BROADCAST`.
* The AppOp carried by the broadcast is checked against the receiving manifest receiver as well
  (`noteOpForManifestReceiver`).

So a hypothetical injector must not only hold the permission; the receiver must also continue to hold
it, and the AppOp must be allowed. The AOSP test app's comment ("signed with platform signature in
order to send test broadcasts") is consistent with this.

### Gate 3 — Permissions

Permission protection levels from `core/res/AndroidManifest.xml`:

| Permission | `protectionLevel` | Relevance |
| --- | --- | --- |
| `android.permission.RECEIVE_EMERGENCY_BROADCAST` | `signature｜privileged` | used as the `receiverPermission` on the CMAS test broadcast |
| `android.permission.RECEIVE_SMS` | `dangerous` (`permissionFlags="hardRestricted"`) | used as the `receiverPermission` on the generic/ETWS test broadcast |
| `android.permission.BROADCAST_SMS` | `signature` | declared by the test app's manifest |
| `android.permission.MODIFY_CELL_BROADCASTS` | `signature｜privileged` | needed to program channels; **not** needed for the test path |
| `android.permission.BROADCAST_CLOSE_SYSTEM_DIALOGS` | `signature｜privileged｜recents` | needed by the receiver to close the shade before the alert |
| `android.permission.START_ACTIVITIES_FROM_BACKGROUND` | `signature｜privileged｜vendorPrivileged｜oem｜verifier｜role` | needed for the full-screen activity from background |

`RECEIVE_EMERGENCY_BROADCAST` is a **privileged** permission: it can only be held by an app in a
priv-app/`system_ext` partition *and* listed in a `privapp-permissions` allowlist. A normal Play
Store app cannot hold it at all.

### Gate 4 — AppOps

The test app passes an AppOp to `sendOrderedBroadcastAsUser`:

* CMAS path: `AppOpsManager.OP_RECEIVE_EMERGECY_SMS`, string `RECEIVE_EMERGENCY_BROADCAST`
* generic path: `AppOpsManager.OP_RECEIVE_SMS`

From `frameworks/base/+/main/core/java/android/app/AppOpsManager.java`:

```java
new AppOpInfo.Builder(OP_RECEIVE_EMERGECY_SMS, OPSTR_RECEIVE_EMERGENCY_BROADCAST,
        "RECEIVE_EMERGENCY_BROADCAST").setSwitchCode(OP_RECEIVE_SMS)
```

AppOps is an additional user-visible toggle layer (Settings → Special app access). For a platform
package it is satisfiable, but it is another gate that must be accounted for.

## 2. What the receiving app itself must hold

`packages/apps/CellBroadcastReceiver/Android.bp`:

```python
java_defaults {
    name: "CellBroadcastDefaults",
    min_sdk_version: "30",
    sdk_version: "module_current",
    privileged: true,
    ...
}

android_app {
    name: "CellBroadcastApp",
    certificate: "networkstack",
    privapp_allowlist: ":privapp_allowlist_com.android.cellbroadcastreceiver.module.xml",
    updatable: true,
}

android_app {
    name: "CellBroadcastAppPlatform",
    certificate: "platform",
    system_ext_specific: true,
    privileged: true,
    privapp_allowlist: ":platform_privapp_allowlist_com.android.cellbroadcastreceiver.xml",
}
```

The actual runtime grants — `apex/permissions/com.android.cellbroadcastreceiver.module.xml`:

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

So the *receiver* is privileged (`privileged: true`) and signed with either the `networkstack` or the
`platform` certificate. Note that `RECEIVE_EMERGENCY_BROADCAST` is in the allowlist — the CBR app is
both a sender and a holder of it in practice (as receiver of `ACTION_SMS_EMERGENCY_CB_RECEIVED`).

## 3. Answering the brief's security questions directly

| Question | Answer | Evidence |
| --- | --- | --- |
| Can an ordinary third-party APK send the emergency broadcast? | **NO** | `BroadcastController` protected-broadcast UID check |
| If no, exactly why? | The action is a `<protected-broadcast>`; the caller's app ID must be one of the system UIDs (or the caller must be a persistent app). A normal app's UID is not on that list. | same |
| Can an ordinary APK construct `SmsCbMessage`? | **Partially / NO in practice.** The class is `@hide` + `@SystemApi` and its public constructors are annotated `@hide`. Compiling against it requires `platform_apis`; at runtime the class is present but the *constructors* are hidden. Even with a hand-built Parcel, Gate 1 blocks delivery. | `SmsCbMessage.java` (`@hide`, `@SystemApi`, `/** @hide */ public SmsCbMessage(...)`) |
| Can ADB send the emergency broadcast? | **NO** — with a documented nuance. The caller is `shell` (UID 2000), which is not in the `isCallerSystem` list above, so `am broadcast` of a protected action from shell throws `SecurityException`. `am broadcast` cannot pin a system UID. **However**, if a *device-side helper* exists that is already running as a system UID, ADB can ask *that helper* to send it. ADB is a transport, not an authority. See Experiment 8. | `BroadcastController`; and the fact that no `cmd`/`service` shell interface for injecting CB exists in either module (searched: no `ShellCommand`, no `onCommand`, no test-injection binder API in `CellBroadcastService` or `CellBroadcastReceiver`) |
| Can shell UID do it? | **NO** for the same UID-listing reason. | same |
| Can root do it? | **YES, most likely — but only for Gate 1.** `ROOT_UID` is explicitly in the `isCallerSystem` list, so a process running as root can send the protected broadcast. What root buys is the *ability to run as a system UID*; it does not by itself grant the `signature｜privileged` permission, nor does it bypass AppOps. Practically, root means you can install a system-signed/priv-app helper or run one as `system`/`phone`. **Status: LIKELY, requires experimental verification.** | `BroadcastController` (`ROOT_UID` case) |
| Can a Magisk module do it? | **LIKELY, by the same mechanism as root** — a Magisk module runs late and can place files in `/system`-derived mounts and execute as root. The module would need to supply a helper that runs as `system`/`phone` and holds the required signature permissions. Magisk does not, by itself, create a platform-signed app. **Status: LIKELY, requires experimental verification.** | reasoning from the above; no Magisk-specific code inspected |
| Can a system-signed APK do it? | **YES, if it is also in a system UID.** Signing alone is necessary but not sufficient: the app must additionally share `android.uid.phone` (or another accepted system UID), which forces the same certificate as the other packages in that UID. | test app manifest + `Android.bp` `certificate: "platform"` |
| Can a privileged APK in `/system/priv-app` do it? | **YES, if it holds the required `signature｜privileged` permission via a privapp allowlist AND runs as an accepted system UID.** A priv-app that is only priv-app (signed with a platform *shared* key but running in its own UID) still fails Gate 1. This is the subtle part most discussions get wrong. | `BroadcastController` UID list; privapp-permissions files |
| Does the build have to be userdebug/eng? | **Depends on what you want.**
    - The protected broadcast / permission path works on **user builds** too (it is not debug-gated).
    - **CBR's "testing mode" is debug-gated by default in the app's logic**, but the AOSP resource
      `allow_testing_mode_on_user_build` is `true`, so on AOSP the secret-code toggle works on a user
      build as well. OEM overlays can set it to `false` (e.g. `values-mcc440-mnc20`).
    - Channels marked `debug_build=true` require `ro.debuggable == 1` (userdebug/eng).
    **So: userdebug/eng is required for debug-only channels and is the safe assumption for a developer
    device, but it is not what gates the core broadcast.** | `CellBroadcastReceiver.java`; `CellBroadcastChannelManager.java`; `res/values/config.xml` |
| Does SELinux matter? | **YES.** Sending a broadcast as `system`/`phone` and writing to the CB history provider are both subject to SELinux. A hand-rolled helper may be denied even when it holds the right UID and permission. **Status: LIKELY to matter, UNKNOWN in detail — requires selinux policy inspection and an experiment.** No SELinux policy files have been inspected yet. | not yet inspected |
| Does the modem have to participate? | **NO for the test path, YES for a real broadcast.** The test path enters at the broadcast boundary, entirely above the RIL. Nothing in `CellBroadcastAlertService` consults the modem. | `aosp-test-path.md` trace |

## 4. The privilege ladder, from weakest to strongest

| Environment | Can reach the genuine pipeline? | Why |
| --- | --- | --- |
| Stock retail device, ordinary APK | **No** | Gates 1 + 3 closed |
| Stock retail device, ADB shell | **No** | Gate 1 closed (`shell` UID not accepted) |
| Stock device + ADB-triggered *existing* system component | **UNKNOWN** | No such injection interface found so far; would need one to exist |
| Rooted stock device | **Likely yes** | `ROOT_UID` accepted; but permission/AppOps/SELinux still to be satisfied |
| Rooted device + Magisk module installing a system helper | **Likely yes** | as above, with a durable installation |
| Custom ROM / AOSP userdebug or eng build | **Yes** | the test app is part of the build; platform signing available |
| AOSP emulator (userdebug GSI / AOSP image) | **Likely yes** | same as above, no OEM overlay interference |
| Device with the AOSP test app built in | **Yes — confirmed by construction** | this is the officially supported configuration |
| Embedded/lab Android device (e.g. a development phone with an eng build) | **Yes** | same |

## 5. Explicit non-answers

These are recorded as unknowns rather than guesses, and each has a corresponding experiment:

* Whether a rooted-but-stock device can pass AppOps for `RECEIVE_EMERGENCY_BROADCAST` without
  additional steps. -> Experiment 7
* Whether SELinux denies a hand-rolled helper that is not part of the platform. -> Experiment 7
* Whether `am broadcast` from `shell` can ever succeed for this action via any flag or
  `--user`/`--receiver-permission` combination. -> Experiment 8
* Whether an OEM (Samsung/Xiaomi) restricts the receiver's declared intent filters or replaces the
  receiver entirely. -> Experiments 10–12
