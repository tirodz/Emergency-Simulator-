# Protocol and alert types

This document covers the concrete message identifiers available for testing, what each one does, and
the specific question of whether a "rocket attack" style alert exists.

All identifiers are quoted from `packages/modules/CellBroadcastService`,
`src/com/android/cellbroadcastservice/SmsCbConstants.java`. None are invented.

## 1. The two message formats

| Format | Constant | Identifier field | Android class |
| --- | --- | --- | --- |
| 3GPP (GSM/UMTS/LTE/5G) | `SmsCbMessage.MESSAGE_FORMAT_3GPP` | 16-bit message identifier = service category | `SmsCbMessage` with `SmsCbEtwsInfo` or `SmsCbCmasInfo` |
| 3GPP2 (CDMA) | `SmsCbMessage.MESSAGE_FORMAT_3GPP2` | service category | `SmsCbMessage` with `SmsCbCmasInfo` |

A GSM/UMTS message identifier is two bytes; a CDMA service category is a 16-bit field. Both are
commonly written `0x111C` style.

## 2. ETWS (Earthquake and Tsunami Warning System)

| Identifier | Constant | Decimal | What Android does |
| --- | --- | --- | --- |
| `0x1100` | `MESSAGE_ID_ETWS_EARTHQUAKE_WARNING` | 4352 | "Earthquake warning" title, `etws_earthquake.ogg` tone |
| `0x1101` | `MESSAGE_ID_ETWS_TSUNAMI_WARNING` | 4353 | "Tsunami warning" title, `etws_tsunami.ogg` tone |
| `0x1102` | `MESSAGE_ID_ETWS_EARTHQUAKE_AND_TSUNAMI_WARNING` | 4354 | "Earthquake and tsunami warning" title |
| `0x1103` | `MESSAGE_ID_ETWS_TEST_MESSAGE` | 4355 | **ETWS test message.** Recognised by a dedicated warning-type bit, not just the identifier. |
| `0x1104` | `MESSAGE_ID_ETWS_OTHER_EMERGENCY_TYPE` | 4356 | "Emergency warning" title, `etws_other_disaster.ogg` tone |

### ETWS's distinctive property

ETWS carries a warning type *inside* the message (`SmsCbEtwsInfo.getWarningType()`):

```
ETWS_WARNING_TYPE_EARTHQUAKE = 0x00
ETWS_WARNING_TYPE_TSUNAMI = 0x01
ETWS_WARNING_TYPE_EARTHQUAKE_AND_TSUNAMI = 0x02
ETWS_WARNING_TYPE_TEST_MESSAGE = 0x03
ETWS_WARNING_TYPE_OTHER_EMERGENCY = 0x04
ETWS_WARNING_TYPE_UNKNOWN = -1
```

`CellBroadcastAlertService.isChannelEnabled()` branches on this directly:

```java
if ((etwsInfo != null && etwsInfo.getWarningType()
        == SmsCbEtwsInfo.ETWS_WARNING_TYPE_TEST_MESSAGE)
        || resourcesKey == R.array.etws_test_alerts_range_strings) {
    return emergencyAlertEnabled
            && CellBroadcastSettings.isTestAlertsToggleVisible(getApplicationContext())
            && checkAlertConfigEnabled(subId, CellBroadcastSettings.KEY_ENABLE_TEST_ALERTS,
            res.getBoolean(R.bool.test_alerts_enabled_default));
}
```

So an ETWS test message is *recognised as a test by the message itself*, and is gated behind the
"Test alerts" toggle (default **off**).

**This is the cleanest "visibly a test" mechanism Android has.** It is worth building the first PoC
around ETWS test rather than CMAS.

## 3. CMAS / WEA identifiers

| Identifier(s) | Class constant | Toggle that gates it | Default |
| --- | --- | --- | --- |
| `0x1112` (+`0x111F` language) | `CMAS_CLASS_PRESIDENTIAL_LEVEL_ALERT` | none — always displayed | always on |
| `0x1113`–`0x1116` (+`0x1120`–`0x1123`) | `CMAS_CLASS_EXTREME_THREAT` | `KEY_ENABLE_CMAS_EXTREME_THREAT_ALERTS` | on |
| `0x1117`–`0x111A` (+`0x1124`–`0x1127`) | `CMAS_CLASS_SEVERE_THREAT` | `KEY_ENABLE_CMAS_SEVERE_THREAT_ALERTS` | on |
| `0x111B` (+`0x1128`) | `CMAS_CLASS_CHILD_ABDUCTION_EMERGENCY` | `KEY_ENABLE_CMAS_AMBER_ALERTS` | on |
| `0x111C` (+`0x1129`) | `CMAS_CLASS_REQUIRED_MONTHLY_TEST` | `KEY_ENABLE_TEST_ALERTS` | **off** |
| `0x111D` (+`0x112A`) | `CMAS_CLASS_CMAS_EXERCISE` | `KEY_ENABLE_EXERCISE_ALERTS` | **off** |
| `0x111E` (+`0x112B`) | `CMAS_CLASS_OPERATOR_DEFINED_USE` | `KEY_OPERATOR_DEFINED_ALERTS` | **off** |
| state/local test | class comes from the channel | `KEY_ENABLE_STATE_LOCAL_TEST_ALERTS` | **off** |

The language variants (`0x111F`+ / 4383+) are the additional-language copies of the same classes.

### Display titles Android picks

From `packages/apps/CellBroadcastReceiver/res/values/strings.xml`:

| String resource | Value |
| --- | --- |
| `cmas_presidential_level_alert` | "Presidential alert" |
| `cmas_extreme_alert` | "Extreme alert" |
| `cmas_severe_alert` | "Severe alert" |
| `cmas_amber_alert` | "AMBER alert" |
| `cmas_required_monthly_test` | "Required Monthly Test" |
| `cmas_exercise_alert` | "Emergency alert (exercise)" |
| `cmas_operator_defined_alert` | "Emergency alert (operator)" |
| `state_local_test_alert` | "State/Local test" |
| `etws_earthquake_warning` | "Earthquake warning" |
| `etws_tsunami_warning` | "Tsunami warning" |
| `etws_earthquake_and_tsunami_warning` | "Earthquake and tsunami warning" |
| `etws_other_emergency_type` | "Emergency warning" |

The **title is Android's**, chosen from the class. The **body is ours** (`SmsCbMessage.getMessageBody()`).
The two are independent.

## 4. Which channels actually display

A detail with large consequences, found in
`CellBroadcastAlertService.handleCellBroadcastIntent()`:

```java
CellBroadcastChannelRange range =
        channelManager.getCellBroadcastChannelRangeFromMessage(message);
...
if (range != null && range.mDisplay == true) {
    if (provider.insertNewBroadcast(message)) {
        startService(alertIntent);
        markMessageDisplayed(message);
    }
} else {
    Log.d(TAG, "ignoring the alert due to configured channels was marked as do not display");
}
```

`CellBroadcastChannelManager.findChannelRange(channel)` returns **null** for a channel that appears in
no configured resource array. Therefore:

> **An alert sent on a channel that is not present in any configured range is silently dropped** — no
> UI, no sound, no notification, no database row. Only the service-state log shows it.

This is a second filter, independent of channel enablement and of user toggles. It means the chosen
test channel **must be one Android already knows about**. Safe choices, present in the AOSP default
configuration:

| Channel | Range array | Default toggle state |
| --- | --- | --- |
| `0x1103` (ETWS test) | `etws_test_alerts_range_strings` | off, but visible |
| `0x111C` (CMAS monthly test) | `required_monthly_test_range_strings` | off, but visible |
| `0x111D` (CMAS exercise) | `exercise_alert_range_strings` | off, and only visible in testing mode |
| `0x111E` (CMAS operator-defined) | `operator_defined_alert_range_strings` | off, and only visible in testing mode |

Note the `additional_cbs_channels_strings`, `emergency_alerts_channels_range_strings`,
`public_safety_messages_channels_range_strings` and `state_local_test_alert_range_strings` arrays are
**empty in AOSP defaults** — those channels are added by carrier/OEM overlays. So on a bare AOSP
device, state/local test alerts have no configured channel at all and will be dropped even if the user
toggle is on. State/local test becomes realistic only on devices whose overlay defines those channels.

## 5. Serial number semantics

* The serial number is a 16-bit per-message counter.
* Android uses it for duplicate detection. In CBS, `CbSendMessageCalculator` decides whether a message
  is new; in CBR, `CellBroadcastContentProvider.insertNewBroadcast(message)` determines whether the
  history row already exists (a duplicate insert does not produce a second alert).
* It is **not** a device serial number and carries no subscriber information.
* For our test tool: a monotonically increasing serial number per test keeps repeated test messages
  distinct; reusing a serial number will make the second one a no-op. This is important for a
  "press the button twice" scenario and should be handled deliberately in the controller.

## 6. Mandatory fields

To construct a usable `SmsCbMessage` through the test mechanism, at minimum:

| Field | Why |
| --- | --- |
| message format | `MESSAGE_FORMAT_3GPP` (or `3GPP2` for CDMA) |
| geographical scope | any value; `0` is used by the AOSP test app |
| serial number | duplicate detection |
| location | the AOSP test app passes `new SmsCbLocation("123456")` |
| service category (identifier) | must resolve to a configured range (§4) |
| language code | e.g. `"en"` |
| **message body** | **must be non-empty** — `shouldDisplayMessage` rejects an empty/null body |
| priority | `MESSAGE_PRIORITY_EMERGENCY` to be treated as emergency |
| ETWS info or CMAS info | selects the alert class and the tone |
| subscription id | `0` in the test app |

The AOSP test app's exact constructor call:

```java
new SmsCbMessage(SmsCbMessage.MESSAGE_FORMAT_3GPP, 0, serialNumber,
        new SmsCbLocation("123456"), serviceCategory, language, body,
        priority, null, cmasInfo, 0, 1);
```

## 7. The "rocket attack" question

**Explicit answer:**

> **Android has no "rocket attack" or "missile attack" alert type, and no alert class named after a
> weapon or attack.** No such identifier exists in `SmsCbConstants`, and no such class exists in
> `SmsCbCmasInfo` or `SmsCbEtwsInfo`.

### But there is a CMAS *category* field, and one of its values is close

`SmsCbCmasInfo` carries a `category` field in addition to the message class. Verified values
(`frameworks/base`, `telephony/java/android/telephony/SmsCbCmasInfo.java`):

```
CMAS_CATEGORY_GEO = 0x00        CMAS_CATEGORY_HEALTH = 0x06
CMAS_CATEGORY_MET = 0x01        CMAS_CATEGORY_ENV = 0x07
CMAS_CATEGORY_SAFETY = 0x02     CMAS_CATEGORY_TRANSPORT = 0x08
CMAS_CATEGORY_SECURITY = 0x03   CMAS_CATEGORY_INFRA = 0x09
CMAS_CATEGORY_RESCUE = 0x04     CMAS_CATEGORY_CBRNE = 0x0a
CMAS_CATEGORY_FIRE = 0x05       CMAS_CATEGORY_OTHER = 0x0b
CMAS_CATEGORY_UNKNOWN = -1
```

**`CMAS_CATEGORY_CBRNE` (Chemical/Biological/Nuclear/Explosive) is the closest thing Android has to a
"rocket attack" alert.** And it is a *displayed* field — `CellBroadcastResources.getCmasCategoryResId()`
maps it to the string resource:

```xml
<string name="cmas_category_cbrne">Chemical/Biological/Nuclear/Explosive</string>
```

under the heading `cmas_category_heading` = "Alert Category:". So a CMAS message with
`CMAS_CATEGORY_CBRNE` will render a line reading "Alert Category: Chemical/Biological/Nuclear/Explosive"
inside Android's genuine alert dialog.

This is the honest, source-backed answer to the brief's §11. It is a *category*, not a dedicated alert
type, and Android's own words for it are "Chemical/Biological/Nuclear/Explosive" — not "rocket attack".
Do not invent an identifier; use this one and let Android render its own wording.

### Full list of CMAS display strings Android can render

`CellBroadcastResources` composes the alert text from category, response type, severity, urgency and
certainty. Response types (`SmsCbCmasInfo.CMAS_RESPONSE_TYPE_*`): `SHELTER` (0x00), `EVACUATE` (0x01),
`PREPARE` (0x02), `EXECUTE` (0x03), `MONITOR` (0x04), `AVOID` (0x05), `ASSESS` (0x06), `NONE` (0x07).

So the *presentation* is richer than the message body alone: Android can assemble a real emergency
alert layout from fields we control, and it will use its own wording for all of them.

### What we can and cannot do

**Can:** choose a genuine class (ETWS test, ETWS other, CMAS extreme/severe, monthly test, exercise,
operator-defined, state/local test); set the CMAS category to `CBRNE` or `SECURITY`; set severity,
urgency, certainty and response type; and author the free-form body text.

**Cannot:** create a "rocket attack" alert class. It does not exist.

**Should not:** pair an alarming category with body text that reads like a live government warning on a
device others might see. The project rule stands: identify the alert as a test wherever the mechanism
permits, and prefix the body with `TEST ALERT — SIMULATION`.

The recommendable "dramatic but honest" configuration is:

```
Class:    ETWS test (0x1103, ETWS_WARNING_TYPE_TEST_MESSAGE)   -> Android itself labels it a test
Body:     "TEST ALERT - SIMULATION / DEVELOPMENT - <free text describing the scenario>"
```

or, if a CMAS layout is wanted:

```
Class:    CMAS EXERCISE (0x111D)   -> "Emergency alert (exercise)"
Category: CMAS_CATEGORY_CBRNE      -> "Chemical/Biological/Nuclear/Explosive"
Body:     "TEST ALERT - SIMULATION / DEVELOPMENT - <free text>"
```

The second option reaches the visual drama (full-screen, alarm tone, vibration, lock-screen wake,
a category line) while the platform's own class is an exercise and the body says it is a test. That is
the honest version of the rocket-attack demo.

## 8. Test categories summary

| Category | Identifier | Android recognises | Special behaviour | Safe for testing | Configurable by us | OEM-dependent |
| --- | --- | --- | --- | --- | --- | --- |
| ETWS test | `0x1103` + warning type 0x03 | yes | dedicated test warning type | **yes, best choice** | yes (toggle + body) | wording overlay only |
| ETWS earthquake | `0x1100` | yes | distinct tone | yes, but alarming | yes | wording |
| ETWS tsunami | `0x1101` | yes | distinct tone | yes, but alarming | yes | wording |
| ETWS other | `0x1104` | yes | generic emergency | yes | yes | wording |
| CMAS monthly test | `0x111C` | yes | "Required Monthly Test" title | **yes** | yes | channel config per carrier |
| CMAS exercise | `0x111D` | yes | exercise title; toggle only visible in testing mode | yes | yes (needs testing mode) | channel config |
| CMAS operator-defined | `0x111E` | yes | operator title | yes | yes (needs testing mode) | channel config |
| CMAS presidential | `0x1112` | yes | cannot be turned off | **no — do not use for tests** | body only | always available |
| CMAS extreme / severe | `0x1113`–`0x111A` | yes | full emergency presentation | only with clearly-labelled body | yes | channel config |
| CMAS AMBER | `0x111B` | yes | child abduction | no — real-world meaning | body only | channel config |
| State/local test | overlay-defined, e.g. `0x112E`/`0x112F` (mcc284, mcc424 overlays) | yes | test title | yes **if the overlay defines a channel** | needs overlay | **yes** |
| Manufacturer/carrier test channels | e.g. `0xA000`–`0xA009` (Korea mcc450-mnc05 overlay), `0xA16A`–`0xA16C` (mcc450-mnc06) | yes if configured | `testing_mode=true` | yes | needs matching overlay | **yes** |

> `testing_mode=true` channels exist only in carrier overlays (UK, Bulgaria, Korea, and others). They
> are listed in `oem-compatibility.md` and are the reason CBR's `*#*#2627#*#*` toggle exists.

## 9. Note on real 3GPP protocol values

This document intentionally reproduces **Android's constants**, which are sourced from AOSP. For the
underlying 3GPP definition of these message identifiers, consult 3GPP TS 23.041. No protocol values
beyond the AOSP constants have been asserted here.
