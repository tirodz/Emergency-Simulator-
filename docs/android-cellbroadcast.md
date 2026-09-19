# Android Cell Broadcast: what it is and how Android models it

This document establishes the vocabulary and the data model. It is deliberately narrow: it covers
what a third-party engineer must understand before reasoning about privileges. The end-to-end flow is
in [`architecture.md`](architecture.md); the security boundary is in
[`privilege-model.md`](privilege-model.md).

## 1. What Cell Broadcast is

Cell Broadcast (CB) is a point-to-multipoint messaging feature of GSM/UMTS/LTE/5G. A base station
transmits a short message to **all** devices camped on a cell (optionally restricted by a message
identifier, i.e. "channel"). Unlike SMS it is not addressed to a subscriber, so there is no per-device
delivery, no acknowledgements, and no subscriber identity involved.

Two technical families are relevant to Android:

| Family | Origin | Android's internal name | Identifier field |
| --- | --- | --- | --- |
| GSM/UMTS Cell Broadcast | 3GPP | `SmsCbMessage.MESSAGE_FORMAT_3GPP` | "message identifier" / service category, 16-bit |
| CDMA Cell Broadcast (CMAS) | 3GPP2 | `SmsCbMessage.MESSAGE_FORMAT_3GPP2` | service category |

A third concept sits alongside them and is what most people actually mean by "emergency alert":

| Family | Purpose |
| --- | --- |
| ETWS (Earthquake and Tsunami Warning System, Japan) | Fast, short warnings; earthquake, tsunami, test, other |
| CMAS / WEA (Commercial Mobile Alert System / Wireless Emergency Alerts, US) | Presidential, extreme, severe, AMBER, monthly test, exercise, operator-defined, public safety, state/local test |
| PWS (Public Warning System) | The umbrella term; Android treats the identifier range 4352–6399 as PWS |

Confirmed in `SmsCbConstants` (`packages/modules/CellBroadcastService`,
`src/com/android/cellbroadcastservice/SmsCbConstants.java`): `MESSAGE_ID_PWS_FIRST_IDENTIFIER` and
`MESSAGE_ID_PWS_LAST_IDENTIFIER` bracket the alert identifier space, and the ETWS identifiers
(`0x1100`–`0x1104` = 4352–4356) and CMAS identifiers (`0x1112` onwards = 4370+) are named constants.

## 2. What Android receives

At the lowest level the framework receives an opaque byte array from the radio interface:

```
ICellBroadcastService.handleGsmCellBroadcastSms(int slotId, byte[] message)
ICellBroadcastService.handleCdmaCellBroadcastSms(int slotId, byte[] bearerData, int serviceCategory)
```

`frameworks/base`, `telephony/java/android/telephony/ICellBroadcastService.aidl` — the AIDL carries
only raw bytes and a slot index. The framework does **not** decode the PDU; decoding happens inside
the CellBroadcastService module.

The module decodes those bytes into a single parcelable object, `android.telephony.SmsCbMessage`, and
that object is the *only* payload that flows onward.

### `SmsCbMessage` fields that matter

From `frameworks/base`, `telephony/java/android/telephony/SmsCbMessage.java` (class is annotated
`@hide` and `@SystemApi`) — the public getters include:

| Getter | Meaning |
| --- | --- |
| `getMessageFormat()` | `MESSAGE_FORMAT_3GPP` (1) or `MESSAGE_FORMAT_3GPP2` (2) |
| `getGeographicalScope()` | PLMN-wide / location-area / cell-wide |
| `getSerialNumber()` | Identifies a *specific* message; used for duplicate detection |
| `getServiceCategory()` | The message identifier / channel number |
| `getLanguageCode()` | ISO-639-1 code carried in the message |
| `getMessageBody()` | **The visible text.** This is a free-form string |
| `getMessagePriority()` | `MESSAGE_PRIORITY_EMERGENCY` or normal |
| `getEtwsWarningInfo()` | `SmsCbEtwsInfo` or null |
| `getCmasWarningInfo()` | `SmsCbCmasInfo` or null |

Two methods drive most downstream behaviour:

```java
public boolean isEmergencyMessage() { return mPriority == MESSAGE_PRIORITY_EMERGENCY; }
public boolean isEtwsMessage()      { return mEtwsWarningInfo != null; }
```

**Consequence for this project:** whether a message is treated as an emergency is decided by
(i) the emergency priority flag and (ii) whether the service category falls inside a configured
channel range — *not* by the text. `getMessageBody()` is free text. The text can therefore say
anything, including something that looks like a missile warning, without changing the alert class.

## 3. ETWS vs CMAS in Android's model

| | ETWS | CMAS |
| --- | --- | --- |
| Carried in | `SmsCbEtwsInfo` | `SmsCbCmasInfo` |
| Discrimination | `message.isEtwsMessage()` | message not ETWS, identifier in CMAS ranges |
| Warning types | earthquake, tsunami, earthquake+tsunami, **test message**, other emergency | message class: presidential, extreme, severe, AMBER, monthly test, exercise, operator-defined, public safety, state/local test |
| Duplicate suppression | ETWS carries a "primary/secondary" flag and a serial number | serial number only |

`SmsCbEtwsInfo` constants (`frameworks/base`): `ETWS_WARNING_TYPE_EARTHQUAKE = 0x00`,
`TSUNAMI = 0x01`, `EARTHQUAKE_AND_TSUNAMI = 0x02`, `ETWS_WARNING_TYPE_TEST_MESSAGE = 0x03`,
`OTHER_EMERGENCY = 0x04`, `UNKNOWN = -1`.

`SmsCbCmasInfo` classes (`frameworks/base`): `CMAS_CLASS_PRESIDENTIAL_LEVEL_ALERT = 0x00`,
`EXTREME_THREAT = 0x01`, `SEVERE_THREAT = 0x02`, `CHILD_ABDUCTION_EMERGENCY = 0x03`,
`REQUIRED_MONTHLY_TEST = 0x04`, `CMAS_EXERCISE = 0x05`, `OPERATOR_DEFINED_USE = 0x06`.

### Important asymmetry

Android's **ETWS** path has an explicit, first-class *test* warning type
(`ETWS_WARNING_TYPE_TEST_MESSAGE`). Android's **CMAS** path has test-like classes (monthly test,
exercise, state/local test) that are distinguished by their *identifier*, not by a dedicated type
bit. This matters because ETWS test handling is keyed directly off the message field, whereas CMAS
test handling is keyed off the channel configuration.

## 4. Message identifiers used by Android

All values below are named constants in `SmsCbConstants`
(`packages/modules/CellBroadcastService`). They are reproduced here because later documents refer to
them; none are invented.

### ETWS

| Constant | Value | Decimal |
| --- | --- | --- |
| `MESSAGE_ID_ETWS_EARTHQUAKE_WARNING` | `0x1100` | 4352 |
| `MESSAGE_ID_ETWS_TSUNAMI_WARNING` | `0x1101` | 4353 |
| `MESSAGE_ID_ETWS_EARTHQUAKE_AND_TSUNAMI_WARNING` | `0x1102` | 4354 |
| `MESSAGE_ID_ETWS_TEST_MESSAGE` | `0x1103` | 4355 |
| `MESSAGE_ID_ETWS_OTHER_EMERGENCY_TYPE` | `0x1104` | 4356 |

### CMAS (GSM/UMTS message identifiers)

| Constant | Value | Decimal |
| --- | --- | --- |
| `MESSAGE_ID_CMAS_ALERT_PRESIDENTIAL_LEVEL` | `0x1112` | 4370 |
| `MESSAGE_ID_CMAS_ALERT_EXTREME_IMMEDIATE_OBSERVED` | `0x1113` | 4371 |
| `MESSAGE_ID_CMAS_ALERT_EXTREME_IMMEDIATE_LIKELY` | `0x1114` | 4372 |
| `MESSAGE_ID_CMAS_ALERT_EXTREME_EXPECTED_OBSERVED` | `0x1115` | 4373 |
| `MESSAGE_ID_CMAS_ALERT_EXTREME_EXPECTED_LIKELY` | `0x1116` | 4374 |
| `MESSAGE_ID_CMAS_ALERT_SEVERE_IMMEDIATE_OBSERVED` | `0x1117` | 4375 |
| `MESSAGE_ID_CMAS_ALERT_SEVERE_IMMEDIATE_LIKELY` | `0x1118` | 4376 |
| `MESSAGE_ID_CMAS_ALERT_SEVERE_EXPECTED_OBSERVED` | `0x1119` | 4377 |
| `MESSAGE_ID_CMAS_ALERT_SEVERE_EXPECTED_LIKELY` | `0x111A` | 4378 |
| `MESSAGE_ID_CMAS_ALERT_CHILD_ABDUCTION_EMERGENCY` | `0x111B` | 4379 |
| `MESSAGE_ID_CMAS_ALERT_REQUIRED_MONTHLY_TEST` | `0x111C` | 4380 |
| `MESSAGE_ID_CMAS_ALERT_EXERCISE` | `0x111D` | 4381 |
| `MESSAGE_ID_CMAS_ALERT_OPERATOR_DEFINED_USE` | `0x111E` | 4382 |
| `MESSAGE_ID_CMAS_ALERT_PRESIDENTIAL_LEVEL_LANGUAGE` | `0x111F` | 4383 |
| `MESSAGE_ID_CMAS_ALERT_EXTREME_IMMEDIATE_OBSERVED_LANGUAGE` | `0x1120` | 4384 |
| …further `_LANGUAGE` variants… | `0x1121`+ | 4385+ |

The `_LANGUAGE` variants are the "additional language" channels (4383–4395) used for the second
emergency-alert language.

## 5. How Android decides to *show* a message

Two independent decisions, which are easy to conflate:

1. **Is this an emergency?** — `SmsCbMessage.isEmergencyMessage()`, plus a channel-range lookup in
   `CellBroadcastChannelManager.isEmergencyMessage()`. If the configured range says
   `emergency=true`, the message is an emergency regardless of the priority bit. If the range does
   not say, the message's own priority bit is the fallback.
2. **Is this channel enabled, and do the user's toggles allow it?** — `shouldDisplayMessage()` in
   `CellBroadcastAlertService`, which walks the resource array the channel belongs to and checks the
   corresponding preference.

Only if both pass does the message reach the database and the alert UI.

The channel-range configuration lives in resource string arrays in
`packages/apps/CellBroadcastReceiver/res/values/config.xml`, in a compact syntax that the channel
manager parses:

```
<string-array name="cmas_alert_extreme_channels_range_strings">
    <item>0x1113-0x1114:rat=gsm, emergency=true</item>
    <item>0x1001:rat=cdma, emergency=true</item>
    <item>0x1120-0x1121:rat=gsm, emergency=true</item>
</string-array>
```

Recognised keys (`CellBroadcastChannelManager.CellBroadcastChannelRange`): `type`, `emergency`,
`rat`, `scope`, `vibration`, `alert_duration`, `override_dnd`, `exclude_from_sms_inbox`,
`display`, `testing_mode`, `always_on`, `screen_on_duration`, `display_icon`,
`dismiss_on_outside_touch`, `debug_build`, `language`, `dialog_with_notification`, `pulsation`,
`filter_language`.

Two of those keys are the reason this project can exist at all:

* `testing_mode=true` — the channel is only processed when the app's testing mode is enabled.
* `debug_build=true` — the channel is dropped entirely unless `ro.debuggable == 1`, i.e. a
  **userdebug/eng build**. Confirmed at
  `CellBroadcastChannelManager.getChannelRangesMapFromResoures()`:
  `if (r.mIsDebugBuildOnly && !mIsDebugBuild) continue;`

## 6. Where the settings live

`CellBroadcastSettings` (in the receiver app) owns the user-facing toggles. The defaults that matter:

| Resource | Default | Meaning |
| --- | --- | --- |
| `master_toggle_enabled_default` | true | master emergency-alert switch |
| `emergency_alerts_enabled_default` | true | umbrella |
| `extreme_threat_alerts_enabled_default` | true | CMAS extreme (extreme threats are on by default) |
| `severe_threat_alerts_enabled_default` | true | CMAS severe |
| `amber_alerts_enabled_default` | true | AMBER |
| `state_local_test_alerts_enabled_default` | **false** | state/local test alerts are **off by default** |
| `test_alerts_enabled_default` | **false** | monthly test / ETWS test are **off by default** |
| `test_exercise_alerts_enabled_default` | **false** | CMAS exercise off by default |
| `test_operator_defined_alerts_enabled_default` | **false** | operator-defined off by default |

`show_test_settings` defaults to `true` and `show_state_local_test_settings` defaults to `true`, so
the toggles are *visible*; they are simply disabled until the user (or the test) turns them on.

**Consequence:** on a stock device, a CMAS monthly-test, exercise, operator-defined, state/local-test
or ETWS-test alert will be *accepted by the pipeline but filtered out at the settings stage* until the
corresponding toggle is enabled. This is a user-visible, non-privileged setting. It is worth an
experiment (see `experiments.md`, Experiment 2).

## 7. Terminology used elsewhere in this repository

* **CBR** — `CellBroadcastReceiver`, package `com.android.cellbroadcastreceiver` or
  `com.android.cellbroadcastreceiver.module`. The app that owns the UI.
* **CBS** — the CellBroadcastService mainline module, package `com.android.cellbroadcastservice`.
  The app that decodes PDUs and emits the broadcast.
* **CB apex** — `com.android.cellbroadcast`, the APEX containing both CBR and CBS as updatable
  mainline modules.
* **Channel / service category / message identifier** — used interchangeably for the 16-bit field
  that selects which alerts a device listens for.
* **Serial number** — a per-message counter used for duplicate detection; *not* a device identifier.
