# The native test-alert surface: what is reachable, and what is not

This document answers one question exhaustively:

> Is there **any** root-free, locally triggerable path on a Galaxy A35 that causes the phone's own
> Cell Broadcast machinery to process a test alert?

It is written against the full AOSP Cell Broadcast surface, re-read from source during this session.
Every claim about AOSP is `CONFIRMED` with the file and line it came from. It was originally written
with every Samsung-firmware claim labelled `UNKNOWN`, because the physical A35 had not been queried.

That gap is now closed for the firmware, though not for the physical handset. On 2026-09-21 the A35's
shipped image for build **A356BXXS4AYD1** (Android 14, One UI 6.1, `ro.build.type=user`,
`release-keys`) was obtained and read directly — the Cell Broadcast APEX, both Samsung RRO overlays,
the telephony and framework jars, the framework-res manifest, `build.prop`, the secret-code parser
and the candidate Samsung packages. §10 records what that reading settled, and the `UNKNOWN`
Samsung rows in §5, §7 and §9 are upgraded to `CONFIRMED (firmware)` there. The physical handset
still has not been queried; the firmware reading answers the *build-level* questions the batch was
built to answer, and the read-only batch remains the way to confirm that a given handset's build
matches the image that was read.

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

**`RESULT B — NO NATIVE ROOT-FREE TEST PATH FOUND`**, previously for the AOSP stack with the
device-specific half open; now for the A35's shipped firmware as well, per §10.

The proof is not "`SMS_CB_RECEIVED` is protected." The proof is that **every** entry point was
enumerated and each fails for its own reason: twelve of them are protected broadcasts reachable only
by a system UID; the one exported service action is `exported="false"`; the two exported activities
and three exported providers read state and cannot inject; the one system property that names itself
"for testing" can only remove messages; there is no shell command; and the OEM secret code is itself
a protected broadcast that opens a display filter rather than originating a message.

The only root-free injection surface is `TEST_TRIGGER_CELL_BROADCAST`, and its single gate is
`ro.debuggable`, which is a build property and cannot be granted. Section 10 confirms that Samsung
adds no second surface and does not remove this gate.

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
| The A35's `ro.debuggable`, package names, manifests and carrier config | `CONFIRMED (firmware)` | §10; image build A356BXXS4AYD1 |
| Whether Samsung kept the 2627 filter or the test receiver | `CONFIRMED (firmware)` | §10; `CellBroadcastReceiver.java` and DRParser in the image |

Prior device evidence from a physical A35 in this repository: **none.** Batch `A35-RO-001` is served
but has never been posted back. The §10 findings are from the *image*, not from a handset, and they
answer the build-level questions that batch was designed to read.

---

## 10. The A35 firmware, read directly (2026-09-21)

The physical handset was never attached, but the shipped image was. Build **A356BXXS4AYD1**
(`UP1A.231005.007`, Android 14, One UI 6.1) was located in a public firmware dump and its Cell
Broadcast-relevant files were read directly: the CB APEX, both Samsung RRO overlays, `build.prop`,
`framework-res.apk`, `telephony-common.jar`, `framework.jar`, `services.jar`, the CB app and service
module, and the candidate Samsung packages (`BCService`, `DRParser`, `ModemServiceMode`,
`serviceModeApp_FB`, `FactoryTestProvider`, `SecFactoryPhoneTest`, `SVCAgent`, `CSC`,
`SecTelephonyProvider`, `EmergencyLauncher`, `EmergencySOS`, `SamsungDialer`, …).

### 10.1 The build gate, `CONFIRMED (firmware)`

```
ro.build.type            = user
ro.build.tags            = release-keys
ro.debuggable            = 0
ro.force.debuggable      = 0
ro.build.version.oneui   = 60100
```

This settles §7's open read. `ro.debuggable=0`, so the AOSP test receiver is never registered. The
`am broadcast` command will still be accepted and print no error, and nothing will happen — the
false-success shape this project exists to catch.

### 10.2 Samsung ships Google's module unmodified, `CONFIRMED (firmware)`

`system/apex/com.google.android.cellbroadcast_compressed.apex` unpacks to Google's stock
`GoogleCellBroadcastApp@341410010` and `GoogleCellBroadcastServiceModule@341410010`. There is no
Samsung-forked Cell Broadcast receiver in the image. The A35's receiver package is
`com.google.android.cellbroadcastreceiver` — the `com.samsung.android.cellbroadcastreceiver` name the
earlier batch looked for is **absent from this build** (it may exist on older One UI versions; that
remains `UNKNOWN` and is recorded as such).

The two Samsung RRO overlays only change presentation:

* `com.google.android.overlay.modules.cellbroadcastreceiver` (target `CellBroadcastCustomization`)
  overrides display strings, one theme, and three UI booleans
  (`show_alert_dialog_with_notification`, `show_alert_speech_setting`,
  `show_presidential_alerts_settings`). It does **not** touch `allow_testing_mode_on_user_build`,
  `show_test_settings`, the channel range arrays, or any receiver.
* `com.google.android.overlay.modules.cellbroadcastservice` (target
  `CellBroadcastServiceCustomization`) sets `cross_sim_duplicate_detection=false` and
  `config_area_info_receiver_packages={com.android.systemui}`. It adds no component.

This is the single most important negative result: a Samsung receiver *fork* would have been the
most likely place for a native trigger to hide, and there is none to hide in.

### 10.3 Samsung did not modify the telephony handlers, `CONFIRMED (firmware)`

`system/framework/telephony-common.jar` decompiles to `GsmInboundSmsHandler` and
`CdmaInboundSmsHandler` with the identical AOSP gate:

```java
TEST_MODE = SystemProperties.getInt("ro.debuggable", 0) == 1;
...
if (TEST_MODE && this.mTestBroadcastReceiver == null) {
    this.mTestBroadcastReceiver = new GsmInboundSmsHandler.GsmCbTestBroadcastReceiver();
    intentFilter.addAction("com.android.internal.telephony.gsm.TEST_TRIGGER_CELL_BROADCAST");
    context.registerReceiver(this.mTestBroadcastReceiver, intentFilter, 2); // RECEIVER_EXPORTED
}
```

`RECEIVER_EXPORTED` (flag value 2), no permission, action unchanged, and the receiver still calls
`mCellBroadcastServiceManager.sendGsmMessageToHandler(...)`. The AOSP analysis in §3 applies to the
A35 verbatim.

### 10.4 The protected-broadcast list on the A35, `CONFIRMED (firmware)`

The device's own `framework-res.apk` manifest lists, under `<protected-broadcast>`:
`android.provider.Telephony.SMS_CB_RECEIVED`,
`android.provider.action.SMS_EMERGENCY_CB_RECEIVED`,
`android.provider.Telephony.SMS_SERVICE_CATEGORY_PROGRAM_DATA_RECEIVED` and
`android.telephony.action.SECRET_CODE`. §2's rows 1–9 hold on this build: AMS refuses these before
broadcast resolution.

### 10.5 The secret code on the A35, `CONFIRMED (firmware)`

Two files settle it. In `DRParser.apk` (`ParseService.java`), `*#*#2627#*#*` is rewritten to the
**protected** `android.telephony.action.SECRET_CODE` (the rewrite is forced for exactly `2627` and
`4636`) and sent as a broadcast — so the platform, not Samsung, is what refuses it. In the CB app
(`CellBroadcastReceiver.java`), the handler is exactly the AOSP one:

```java
if ("android.telephony.action.SECRET_CODE".equals(action)) {
    if (SystemProperties.getInt("ro.debuggable", 0) == 1
            || resources.getBoolean(R.bool.allow_testing_mode_on_user_build)) {
        setTestingMode(!isTestingMode(this.mContext));
        ...
```

And the shipped `res/values/bools.xml` reads `<bool name="allow_testing_mode_on_user_build">true</bool>`
(not overridden by the RRO). So on the A35 the 2627 toggle *is* live on a `user` build — and it still
only flips a display filter. It never constructs a message. §4 is confirmed for the A35, gate and
all.

### 10.6 What each Samsung candidate turned out to be, `CONFIRMED (firmware)`

| Package | Looked like | Is |
|---|---|---|
| `com.sec.bcservice` (BCService) | "BC" = Cell Broadcast | A tcpdump/issue-tracker logging service on a unix socket; its only action is `com.sec.android.ISSUE_TRACKER_ONOFF`, `signatureOrSystem` |
| `com.sec.android.app.parser` (DRParser) | a secret-code injector | The keystring router; routes to broadcast actions and `com.samsung.android.cmd`-style handoffs, never constructs a CB message |
| `com.samsung.android.telephony.SemSmsCbMessage` | a Samsung CB API | A read-only `Parcelable` wrapper over `SmsCbMessage`; every method a getter |
| `com.android.phone` (TeleService) | could send CB | Only `get/setCellBroadcastIdRanges`, both requiring signature `MODIFY_CELL_BROADCASTS`; configure ranges, cannot inject |
| `SecFactoryPhoneTest`, `ModemServiceMode`, `serviceModeApp_FB`, `FactoryTestProvider`, `SVCAgent` | factory test injectors | Test activities, RMS/keystring interfaces, provider reads; none constructs a CB message or calls the alert service |
| `EmergencyLauncher` | ETWS handler | Reacts to an ETWS *state flag* for the emergency UI; a consumer, not an injector |
| `com.samsung.rmt_exercise` | a remote-exercise injector | **does not exist in this build** |

A token sweep of every shipped Samsung component and framework jar for
`TEST_TRIGGER_CELL_BROADCAST`, `pdu_string`, `sendGsmMessageToHandler`, `handleCellBroadcastIntent`,
`SmsCbMessage` construction and `CellBroadcastAlertService` returned **no hit outside Google's own
module**. There is no OEM injector in this image to find.

### 10.7 What §10 does not do

It does not prove anything about a *specific handset*. If the operator's A35 has taken a different
update, the build id differs and these facts must be re-read for that build. It also does not
demonstrate delivery even on `ro.debuggable=1`: the firmware reading closes the "does a gate exist"
question, not the "does an alert appear" question, which is still logcat evidence on a device.

### 10.8 Reproducibility

The facts above are encoded in `src-tauri/src/platform.rs` as `samsung_firmware_facts()`, each row
carrying its `FirmwareEvidence` label and the firmware path it was read from, and are rendered in the
diagnostics report. That is deliberate: the claims live in code with tests, so a later edit cannot
quietly upgrade a firmware fact to a device observation.

---

## 11. The non-broadcast surface, walked this session

§10 answered "is the AOSP test receiver present" — it is not, because `ro.debuggable=0`. It did not
answer "is there any *other* interface into the Cell Broadcast machinery", because §10 only looked at
broadcasts. This section walks the classes a broadcast matrix cannot see: shell commands, Binder
services, the platform `ITelephony` service, `system_server`, content providers and the exported OEM
components. Each one is recorded with the reason it does or does not work, in
`platform::interface_candidates()`.

### 11.1 The pipeline has exactly one producer, and it is not a broadcast

Reading Google's unmodified Cell Broadcast module
(`/apex/com.android.cellbroadcast/.../GoogleCellBroadcastServiceModule@341410010`) end to end gives
the complete data flow:

```
telephony radio callback  ─┬─►  ICellBroadcastService.handleGsmCellBroadcastSms(phoneId, byte[])
telephony test receiver   ─┘              │
                                          ▼
                          DefaultCellBroadcastService.onGsmCellBroadcastSms
                                          │  decodes the PDU into an SmsCbMessage
                                          ▼
                          GsmCellBroadcastHandler.handleBroadcastSms
                                          │  inserts a row, then broadcasts
                                          ▼
                   CellBroadcastIntents.sendSmsCbReceivedBroadcast
                                          │  act=android.provider.Telephony.SMS_CB_RECEIVED
                                          │  pkg=com.android.cellbroadcastreceiver
                                          ▼
                          CellBroadcastReceiver ─► CellBroadcastAlertService
                                          │  the channel-range / testing-mode / language gates
                                          ▼
                          CellBroadcastAlertDialog  (the native alert)
```

There are exactly two callers of `CellBroadcastServiceManager.sendGsmMessageToHandler` in the entire
image: the GSM test receiver and the CDMA path. Both are inside the module, both are gated on
`ro.debuggable`. Everything above `sendSmsCbReceivedBroadcast` is fed by one of those two, and
everything below it is payload-neutral — each downstream hop only holds an `SmsCbMessage`, which has
no public constructor. That is why no downstream component can be used as an injector: there is no
way to hand one a message.

### 11.2 `cmd cellbroadcast` does not exist

The Cell Broadcast module implements no `ShellCommand` at all. `DefaultCellBroadcastService` has
`onGsmCellBroadcastSms`, `onCdmaCellBroadcastSms`, `onCdmaScpMessage`, `getCellBroadcastAreaInfo`
and a `dump()` — and nothing else. There is no verb surface to find. The full verb list of the one
relevant command, `cmd phone` (read out of `TeleService.apk`), is: `ims`, `uce`, `cc`, `gba`, `src`,
`d2d`, `data`, `radio`, `euicc`, `barring`, `emergency-number-test-mode`, `emergency-callback-mode`,
`thermal-mitigation`, `restart-modem`, `unattended-reboot`, `get-imei`, `numverify`. Not one
constructs an `SmsCbMessage` or calls the alert service.

### 11.3 The MockModem route: the strongest near-miss, and why it fails twice

`cmd phone radio set-modem-service mockmodem` is the one verb that *would* work: `MockModemService`
replays RIL events, including broadcast SMS, through the real `mCi` callback, which is the radio arm
of the diagram above. It fails for two independent reasons:

* the `com.android.telephony.mockmodem` package is **not present in the A35 image** (`MOCKMODEM 0`),
  so there is no service to select; and
* selecting any modem service calls `ITelephony.setModemService`, enforced on the signature
  permission `android.permission.MODIFY_PHONE_STATE`, which `shell` does not hold and cannot be
  granted.

`cmd phone carrier_restriction_status_test` is also gated on MockModem being the active service.

### 11.4 The `phone` service has only configuration, not injection

The only Cell Broadcast methods on `ITelephony` are `getCellBroadcastIdRanges` and
`setCellBroadcastIdRanges`, both enforcing the signature permission `MODIFY_CELL_BROADCASTS`, and
both only configure which channel ranges are enabled. `updateEmergencyNumberListTestMode` is a real
test-mode setter for the emergency *number* database, not for Cell Broadcast.

### 11.5 The Samsung CMAS rows are real, and they are storage, not an injector

Samsung's telephony provider stores emergency alerts as SMS rows whose address is `#CMAS#`,
`#CMAS#Test`, `#CMAS#Presidential`, `#Emergency Alert#Amber` and so on, with a literal `cmas` table
and `address LIKE '#CMAS#%'` cleanup, and the comment `CMAS messages are not allowed by FCC rule`.
These are the SMS database's representation of an alert the platform already produced. There is no
exporter that turns a caller-supplied row back into an alert — the rows are written *by* the alert
path, downstream of it.

### 11.6 `system_server` has no producer

A sweep of `services.jar` for `SmsCbMessage` construction and for the alert actions found exactly one
touch: `com.att.iqi.libs.CellBroadcastObserver`, an observer that reports Cell Broadcast activity to
the AT&T IQI diagnostics library. It consumes messages; it cannot originate one.

### 11.7 The conclusion, restated honestly

The real injector is `ICellBroadcastService.handleGsmCellBroadcastSms` — the method that decodes a
raw broadcast PDU into an `SmsCbMessage` and hands it to the genuine alert service — and the only
thing standing in front of it is `ro.debuggable`, not a permission. That is `RESULT B` again, arrived
at from the Binder layer rather than the broadcast layer, but now it is *sharper*: the path is closed
by one build property, and it is closed identically for every non-broadcast interface class. On a
`userdebug` A35 the same trigger this tool already builds — the `pdu_string` broadcast — would reach
`handleGsmCellBroadcastSms` and drive the real pipeline.

That is the honest end state on a retail A35: no root-free path exists, the reason is a build
property rather than a missing permission or a Samsung lockout, and the tool now proves that class by
class rather than asserting it.

