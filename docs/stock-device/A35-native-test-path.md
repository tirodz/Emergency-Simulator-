# The native test-alert surface: what is reachable, and what is not

This document answers one question exhaustively:

> Is there **any** root-free, locally triggerable path on a Galaxy A35 that causes the phone's own
> Cell Broadcast machinery to process a test alert?

It is written against the full AOSP Cell Broadcast surface, re-read from source during this session.
Every claim about AOSP is `CONFIRMED` with the file and line it came from. Every claim that depends
on Samsung's firmware is labelled `UNKNOWN`, because **the physical A35 has never been queried** —
`evidence/` is empty and `ro.debuggable` has not been read. This document does not fill that gap with
inference.

It supersedes nothing. It is the complete candidate enumeration the earlier
`why-adb-cannot-send-sms-cb-received.md` did not attempt: that document settled one path, this one
walks the entire surface.

---

## 1. Method

Two sources only:

1. **AOSP source read this session** (`android14-release`, with `android15-release` and
   `android16-release` checked for drift) — `packages/apps/CellBroadcastReceiver`,
   `packages/modules/CellBroadcastService`, `frameworks/opt/telephony`,
   `frameworks/base/core/res/AndroidManifest.xml`.
2. **Prior device evidence in this repository** — which is, at present, none. Recorded as `UNKNOWN`
   rather than assumed either way.

The candidates are graded on one test: **does the path cause `CellBroadcastAlertService` to process a
message, without the caller holding a system UID?** A path that only changes what is displayed, or
only changes a filter, is not an entry point.

---

## 2. The complete entry-point matrix

The installed Cell Broadcast app has exactly one receiver that accepts alert-bearing intents, and it
is `exported="true"` with no permission:

```xml
<receiver android:name="com.android.cellbroadcastreceiver.CellBroadcastReceiver"
    android:exported="true">
```

Exported is necessary but nowhere near sufficient. `ActivityManagerService` refuses a protected
broadcast from any non-system UID *before* broadcast resolution runs, so the permissionless
`exported` flag never gets a chance to matter. Classifying by `exported` alone is the trap this
matrix exists to avoid.

| # | Entry point | Exported | Protected broadcast | Shell (uid 2000) can send | Reaches alert pipeline | Verdict |
|---|---|---|---|---|---|---|
| 1 | `android.provider.action.SMS_EMERGENCY_CB_RECEIVED` | yes | **yes** | no | would | `BLOCKED` — protected |
| 2 | `android.provider.Telephony.SMS_CB_RECEIVED` | yes | **yes** | no | would | `BLOCKED` — protected |
| 3 | `...SMS_SERVICE_CATEGORY_PROGRAM_DATA_RECEIVED` | yes | **yes** | no | no (config) | `BLOCKED` — protected |
| 4 | `android.telephony.action.DEFAULT_SMS_SUBSCRIPTION_CHANGED` | yes | **yes** | no | no (config) | `BLOCKED` — protected |
| 5 | `android.telephony.action.CARRIER_CONFIG_CHANGED` | yes | **yes** | no | no (config) | `BLOCKED` — protected |
| 6 | `android.intent.action.SERVICE_STATE` | yes | **yes** | no | no (config) | `BLOCKED` — protected |
| 7 | `android.intent.action.LOCALE_CHANGED` | yes | **yes** | no | no (config) | `BLOCKED` — protected |
| 8 | `android.intent.action.BOOT_COMPLETED` | yes | **yes** | no | no (config) | `BLOCKED` — protected |
| 9 | `android.telephony.action.SECRET_CODE` (`2627`) | yes | **yes** | no | **no** — toggles a flag only | `BLOCKED` — protected, and not an injector |
| 10 | `com.android.internal.telephony.gsm.TEST_TRIGGER_CELL_BROADCAST` | *dynamic* | **no** | **yes** | **yes** | reachable iff `ro.debuggable=1` |
| 11 | `com.android.internal.telephony.cdma.TEST_TRIGGER_CELL_BROADCAST` | *dynamic* | **no** | **yes** | **yes** | reachable iff `ro.debuggable=1` |
| 12 | `com.android.internal.telephony.cdma.TEST_TRIGGER_SCP_MESSAGE` | *dynamic* | **no** | **yes** | **yes** | reachable iff `ro.debuggable=1` |
| 13 | `cellbroadcastreceiver.SHOW_NEW_ALERT` | n/a — service | n/a | no — `exported="false"` | would | `BLOCKED` — not exported |
| 14 | `CellBroadcastAlertDialog` (`SMS_CB_RECEIVED` filter) | **no** | n/a | no | would | `BLOCKED` — not exported |
| 15 | `CellBroadcastListActivity` | yes | n/a | launch only | no — reads history | `NOT AN ENTRY POINT` |
| 16 | `CellBroadcastSettings` | yes | n/a | launch only | no — preferences | `NOT AN ENTRY POINT` |
| 17 | `CellBroadcastContentProvider` | yes | n/a | read with `READ_CELL_BROADCASTS` | no — history DB | `NOT AN ENTRY POINT` |
| 18 | `CellBroadcastProvider` (module) | yes | n/a | read with `READ_CELL_BROADCASTS` | no — history DB | `NOT AN ENTRY POINT` |
| 19 | `DefaultCellBroadcastService` (module) | yes | n/a | bind needs `BIND_CELL_BROADCAST_SERVICE` (signature) | no | `BLOCKED` — signature |
| 20 | `CellBroadcastSearchIndexableProvider` | yes | n/a | needs `READ_SEARCH_INDEXABLES` | no | `BLOCKED` — permission |
| 21 | `persist.cellbroadcast.message_filter` (system property) | n/a | n/a | cannot `setprop` a `persist.*` | no — only filters | `NOT AN ENTRY POINT` |
| 22 | shell command (`cmd cellbroadcast` / telephony test verb) | n/a | n/a | **does not exist** | n/a | `NOT PRESENT` |
| 23 | OEM/user-facing "trigger a test alert" setting | n/a | n/a | n/a | n/a | **does not exist in AOSP** |

Points 21–23 are worth stating because each looks like a route until it is read:

* `persist.cellbroadcast.message_filter` is labelled `// Key for accessing message filter from
  SystemProperties. For testing use.` — but it only *removes* messages from consideration. It cannot
  add one, and `persist.*` properties are not writable by shell in any case.
* There is no `cmd`-style shell surface for Cell Broadcast in either module's manifest.
* The "Test alerts" toggle in Settings is a **receive-side preference**, not a trigger. See §5.

---

## 3. Why the guarded path works, and only that one

If `ro.debuggable == 1`, `GsmInboundSmsHandler` registers a receiver at construction:

```java
private static final boolean TEST_MODE = SystemProperties.getInt("ro.debuggable", 0) == 1;
private static final String TEST_ACTION =
        "com.android.internal.telephony.gsm.TEST_TRIGGER_CELL_BROADCAST";
...
if (TEST_MODE) {
    if (mTestBroadcastReceiver == null) {
        mTestBroadcastReceiver = new GsmCbTestBroadcastReceiver();
        IntentFilter filter = new IntentFilter();
        filter.addAction(TEST_ACTION);
        context.registerReceiver(mTestBroadcastReceiver, filter, Context.RECEIVER_EXPORTED);
    }
}
```

Three properties make this the only root-free route:

1. **The action is not a protected broadcast.** It does not appear in the platform's
   `<protected-broadcast>` list, so the `isCallerSystem` check does not apply.
2. **It is registered `RECEIVER_EXPORTED` with no permission**, so uid 2000 reaches it.
3. **It feeds the real handler** — `mCellBroadcastServiceManager.sendGsmMessageToHandler(m)` — the
   same path a radio notification uses, so the genuine pipeline runs downstream.

The gate is `ro.debuggable`, read once at class initialisation. It is a build property. It cannot be
granted, and `adb root` does not change it.

AOSP documents its own invocation in the source comment:

```
adb shell am broadcast -a com.android.internal.telephony.gsm.TEST_TRIGGER_CELL_BROADCAST \
--es pdu_string  0000110011010D0A5BAE57CE770C531790E85C716CBF3044573065B9306757309707767 \
A751F30025F37304463FA308C306B5099304830664E0B30553044FF086C178C615E81FF09000000000000000 \
0000000000000 --ei phone_id 0
```

`--ei phone_id 0` is AOSP's own documented form. This project pins `phone_id` nowhere in the
send path, which is correct: pinning an invalid id lets `am` accept the broadcast and the receiver
then drop it.

---

## 4. The secret code 2627 — what it actually is

This is the most-misread mechanism in the project's history, and BUG-002 is its record.

The filter is real and registered on an exported component:

```xml
<intent-filter>
    <action android:name="android.telephony.action.SECRET_CODE" />
    <!-- CMAS: To toggle test mode for cell broadcast testing on userdebug build -->
    <data android:scheme="android_secret_code" android:host="2627" />
</intent-filter>
```

`2627` is the **numeric host**; the letters are a keyboard mnemonic for `CMAS` (`2=ABC`, `6=MNO`,
`2=ABC`, `7=PQRS`). Google's own module documentation gives the same code in mnemonic form:

> To enable the CMAS secret code, `##CMAS##` (`##2627##` on the dial pad), a dialer app must listen
> for the special dialer code in the form of `##code##` and handle the code using the public method
> `sendDialerSpecialCode`.

Three facts decide it, and each independently kills it as an injection route:

1. **It is a protected broadcast.** `android.telephony.action.SECRET_CODE` is in the platform's
   protected list (line 564 of `frameworks/base/core/res/AndroidManifest.xml`). `am broadcast` from
   shell is refused with `SecurityException` **before any receiver runs**.
   *This means BUG-002's documented reproduction command could never have reproduced anything.* The
   `am broadcast -a android.telephony.action.SECRET_CODE` in that bug report is refused by the
   platform. The bug's conclusion (a toggle is not idempotent) is still true of the handler and the
   fix is still correct, but the reproduction was not a real observation. Corrected here.

2. **It is a gate-opener, not an injector.** The handler does exactly one thing:

   ```java
   } else if (TelephonyManager.ACTION_SECRET_CODE.equals(action)) {
       if (SystemProperties.getInt("ro.debuggable", 0) == 1
               || res.getBoolean(R.bool.allow_testing_mode_on_user_build)) {
           setTestingMode(!isTestingMode(mContext));
   ```

   It flips `testing_mode`. It never constructs an `SmsCbMessage`, never calls
   `CellBroadcastAlertService`, and never presents anything. It changes whether a *test-mode channel
   range* will be displayed when a message arrives later.

3. **The gate it opens is downstream of injection.** `isTestingMode` is consulted in
   `CellBroadcastAlertService` at the channel-range check — it filters a message that already
   arrived. Opening it does not cause a message to arrive.

So 2627 is a preference toggle that a **dialer** can legitimately set. It is not, and cannot be, a
way to make the phone display a test alert on its own.

---

## 5. "Test alerts" in Settings is not a trigger

Mission 6 asks whether a user-facing test function exists. It does, in a narrow sense, and it is not
what it sounds like.

`CellBroadcastSettings.KEY_ENABLE_TEST_ALERTS` (`enable_test_alerts`) is a `SwitchPreference`. Its
visibility is computed from carrier configuration:

```java
public static boolean isTestAlertsToggleVisible(Context context, String operator) {
    ...
    return (res.getBoolean(R.bool.show_test_settings)
            || CellBroadcastReceiver.isTestingMode(context))
            && isTestAlertsAvailable;
}
```

The comment on the key states its meaning exactly: *"Whether to display monthly test messages
(default is disabled)."* It controls whether a test broadcast **that already arrived** is displayed.
It does not send one.

The Google documentation that surfaces this toggle is equally explicit about the direction of
control:

> Occasionally, you may get an alert but not feel an earthquake in your location.
> …
> 5. In your phone's Settings app, tap Notifications and then Wireless emergency alerts.
> 6. Turn Test alerts on or off.

"Turn Test alerts on or off" is a preference change. Nothing in the platform UI originates an alert.

`show_test_settings` defaults to `true`, and `allow_testing_mode_on_user_build` defaults to `true`,
**on Android 14, 15 and 16 alike** — verified on all three branches this session. That means an
OEM that wanted to expose the 2627 toggle on a production build need only leave those resources
alone. It is the OEM's choice to make, and Samsung's choice on the A35 is `UNKNOWN`.

What is *not* true, and must not be written anywhere: that a reachable 2627 toggle would give a
root-free way to display a test alert. It would not, for the reason in §4.3.

---

## 6. Android version matrix

| Item | Android 14 | Android 15 | Android 16 |
|---|---|---|---|
| `ro.debuggable` gate on `TEST_MODE` | present | present | present |
| `TEST_TRIGGER_CELL_BROADCAST` unprotected | yes | yes | yes |
| `SMS_CB_RECEIVED` protected | yes | yes | yes |
| `SECRET_CODE` protected | yes | yes | yes |
| 2627 filter on exported receiver | present | present | present |
| `allow_testing_mode_on_user_build` default | `true` | `true` | `true` |
| `show_test_settings` default | `true` | `true` | `true` |
| Shell identity (`SHELL_UID` 2000) | unchanged | unchanged | unchanged |

No version difference opens a route. The mechanism is stable across the three branches, which is why
"update Android" is not an answer either.

---

## 7. What this means for the A35

The A35's firmware either registers the test receiver or it does not, and exactly one read settles
it:

```
adb shell getprop ro.debuggable
```

* `1` → the telephony test receiver exists; Path A is available; a controlled send can be attempted
  with downstream logcat as the sole verdict.
* `0` → the receiver is never constructed. `am broadcast` will still **accept** the command and print
  no error, and nothing will happen. That is the false-success shape this project exists to catch.
  On `0` the conclusion is `RESULT B`: no native root-free test path, with the local simulator kept
  as a separately-named UI-only fallback.
* empty / unreadable → `UNKNOWN`. Do not assume absence.

The competing hypothesis — that Samsung modified this stack — is also `UNKNOWN`, and the read-only
batch `A35-RO-001` is built to settle both in one pass:

* task 14 reads `ro.debuggable`, the RRO overlays that could carry `config_disable_all_cb_messages`,
  and the boot state;
* task 15 reads whether the 2627 filter survived on `com.samsung.android.cellbroadcastreceiver`;
* task 9 reads whether any test/diagnostic package exists;
* task 16 reads the carrier-side channel ranges.

Nothing in this document upgrades those to `CONFIRMED`.

---

## 8. Final classification

**`RESULT B — NO NATIVE ROOT-FREE TEST PATH FOUND`**, for the AOSP stack, with the device-specific
half still open.

The proof is not "`SMS_CB_RECEIVED` is protected." The proof is that **every** entry point was
enumerated and each fails for its own reason: twelve of them are protected broadcasts reachable only
by a system UID; the one exported service action is `exported="false"`; the two exported activities
and three exported providers read state and cannot inject; the one system property that names itself
"for testing" can only remove messages; there is no shell command; and the OEM secret code is itself
a protected broadcast that opens a display filter rather than originating a message.

The only root-free injection surface is `TEST_TRIGGER_CELL_BROADCAST`, and its single gate is
`ro.debuggable`, which is a build property and cannot be granted.

### Strongest legitimate fallback

* If `ro.debuggable=1`: the controlled test broadcast, verdict from logcat, no root.
* Otherwise: the bundled local simulator, presented as **UI-only** and never as a Cell Broadcast —
  the distinction the README and the CI honesty gate now enforce.

### What would change this answer

A Samsung-specific exported injector that AOSP does not contain, or a `ro.debuggable=1` build. Both
are read-only reads away.

---

## 9. Evidence index

| Claim | Label | Source |
|---|---|---|
| `TEST_MODE` gated on `ro.debuggable`; action registered `RECEIVER_EXPORTED`; feeds `sendGsmMessageToHandler` | `CONFIRMED` | `frameworks/opt/telephony/src/java/com/android/internal/telephony/gsm/GsmInboundSmsHandler.java` lines 53–73, 83–110 |
| CDMA and SCP equivalents exist with the same gate | `CONFIRMED` | `.../cdma/CdmaInboundSmsHandler.java` lines 70–75, 140–151 |
| `TEST_TRIGGER_CELL_BROADCAST` is not protected | `CONFIRMED` | absent from `frameworks/base/core/res/AndroidManifest.xml` protected list |
| `SMS_CB_RECEIVED`, `SMS_EMERGENCY_CB_RECEIVED`, `SECRET_CODE`, `SERVICE_STATE`, `CARRIER_CONFIG_CHANGED`, `LOCALE_CHANGED`, `BOOT_COMPLETED`, `DEFAULT_SMS_SUBSCRIPTION_CHANGED`, `SMS_SERVICE_CATEGORY_PROGRAM_DATA_RECEIVED` are protected | `CONFIRMED` | `frameworks/base/core/res/AndroidManifest.xml` lines 564, 749–752, and grep across the protected list |
| `CellBroadcastReceiver` is `exported="true"` with no permission and carries the 2627 filter | `CONFIRMED` | `packages/apps/CellBroadcastReceiver/AndroidManifest.xml` lines 146–165 |
| `CellBroadcastAlertService`, `CellBroadcastAlertDialog` are `exported="false"` | `CONFIRMED` | same manifest; `AndroidManifest_Platform.xml` |
| The 2627 handler only calls `setTestingMode(!isTestingMode(...))` | `CONFIRMED` | `CellBroadcastReceiver.java` lines 194–212 |
| `isTestingMode` is consulted only at the channel-range filter | `CONFIRMED` | `CellBroadcastAlertService.java` lines 318–325 |
| `KEY_ENABLE_TEST_ALERTS` is described as a display preference | `CONFIRMED` | `CellBroadcastSettings.java` lines 126–127 |
| `isTestAlertsToggleVisible` is gated on carrier ranges and testing mode | `CONFIRMED` | `CellBroadcastSettings.java` lines 905–932 |
| `allow_testing_mode_on_user_build` and `show_test_settings` default `true` on 14/15/16 | `CONFIRMED` | `CellBroadcastReceiver/res/values/config.xml` line 69 (all three branches) |
| `persist.cellbroadcast.message_filter` only filters | `CONFIRMED` | `CellBroadcastAlertService.java` lines 148–151, 328–333 |
| CMAS secret code is OEM-gated and dialer-handled | `CONFIRMED` (official docs) | https://source.android.com/docs/core/ota/modular-system/cellbroadcast |
| "Test alerts" is a user toggle, not a sender | `CONFIRMED` (official docs) | https://support.google.com/android/answer/9319337 |
| The A35's `ro.debuggable`, package names, manifests and carrier config | `UNKNOWN` | not read; `evidence/` is empty |
| Whether Samsung kept the 2627 filter or the test receiver | `UNKNOWN` | requires task 15 / task 14 output |

Prior device evidence in this repository: **none for the physical A35.** Batch `A35-RO-001` is
served but has never been posted back.
