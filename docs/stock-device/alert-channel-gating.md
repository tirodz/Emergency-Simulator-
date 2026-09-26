# The alert channel is a gate, not a label

This document records a defect in the controller, found by reading the receive-side source rather
than the send-side source. It is the mirror image of the mistake this project already documents:
instead of emitting a command that does nothing, the tool emitted a **valid** command carrying the
one channel that a default phone is configured to discard.

Everything about AOSP below is `CONFIRMED` against a named file and line. Anything that depends on
Samsung's firmware is marked `UNKNOWN` and was not assumed.

---

## 1. What the controller did

`send_platform_test_alert` built every PDU with `MESSAGE_ID_ETWS_TEST` — `0x1103`, the ETWS test
message. The interface stated it as fact, in a metric reading `4355 / 0x1103 · locked`, and the
confirmation dialog said the same. The channel was a constant with no way to change it.

The encoding was correct. `0x1103` is the identifier AOSP's own test receiver is built around, and
the reference command in `GsmInboundSmsHandler` uses it. But the write side and the read side are
different halves of the system, and only the write side had been checked.

---

## 2. What the receive side actually does

`CellBroadcastAlertService.isChannelEnabled` (line 497) decides whether a decoded message becomes an
alert. It resolves the message's service category to one of the range arrays in `res/values/config.xml`
and then consults a *different* user preference per array:

| Channel | Range array | Preference checked | Compiled default |
|---|---|---|---|
| `0x1100` ETWS primary | `etws_alerts_range_strings` (line 527) | master toggle only | **on** |
| `0x1113` CMAS extreme | `cmas_alert_extreme_channels_range_strings` (line 567) | master + `KEY_ENABLE_CMAS_EXTREME_THREAT_ALERTS` | **on** |
| `0x1115`–`0x111A` CMAS severe | `cmas_alerts_severe_range_strings` (line 571) | master + `KEY_ENABLE_CMAS_SEVERE_THREAT_ALERTS` | **on** |
| `0x111C` monthly test | `required_monthly_test_range_strings` (line 592) | master + `KEY_ENABLE_TEST_ALERTS` | **off** |
| `0x1103` ETWS test | `etws_test_alerts_range_strings` (line 520) | master + `KEY_ENABLE_TEST_ALERTS` | **off** |

The defaults come from `res/values/config.xml`: `master_toggle_enabled_default` is `true`,
`test_alerts_enabled_default` is `false`.

So `0x1103` is off on a device nobody has configured. `evaluateChannelForTests` also applies a
second filter at line 320:

```java
if (range != null && range.mTestMode && !CellBroadcastReceiver.isTestingMode(mContext)) {
    Log.d(TAG, "ignoring the alert due to not being in testing mode");
    return false;
}
```

and `0x1103` is declared `testing_mode` in the `etws_test_alerts_range_strings` array. That means the
ETWS test channel requires testing mode *plus* the test-alerts toggle.

### The failure shape

This is the defect class the project exists to catch, with one extra turn of the screw. The PDU is
well-formed, `am broadcast` exits 0, the telephony test receiver genuinely fires, and the message
genuinely reaches `CellBroadcastAlertService` — which then discards it as disabled. Downstream the
controller's stage ladder sees no alert and honestly reports `UNCERTAIN`.

So the controller was not lying. It was reporting "nothing observable happened", correctly, while
pointing the operator at a channel that could not have produced anything until they found a settings
toggle the tool never mentioned. On a device that *is* in testing mode with test alerts on, `0x1103`
works — which is why a `userdebug` bench session would have passed and hidden this.

---

## 3. What was changed

`src-tauri/src/platform.rs` now carries `ALERT_CHANNELS`, a catalogue in which every entry states its
message identifier, its receive-side gate, its default state, and what the operator must change.
`cb_pdu(message_id, body, serial)` refuses any identifier that is not in the catalogue, so a PDU
cannot be built for a channel whose gating nobody has checked.

The default is now `0x1100` (ETWS primary): it is a genuine emergency-class alert, it needs no setup
on a default device, and the ETWS test channel is still available and still labelled as needing
setup. The frontend loads the catalogue from the backend through `list_alert_channels`, so the panel
cannot drift from the encoder, and the confirmation dialog states the gate for the channel actually
selected.

The refactor is provably behaviour-preserving for `0x1103`: `the_refactor_preserved_the_etws_test_encoding`
asserts the catalogue-built PDU is byte-identical to the previous single-channel builder.

---

## 4. Honesty boundaries

* **`0x1100` on the A35 is `INFERRED`, not `CONFIRMED`.** The only route that produces a `0x1100`
  PDU is the AOSP telephony test receiver, which is gated on `ro.debuggable=1`. On a retail A35 the
  injected PDU cannot reach the pipeline at all, so the channel default does not by itself create a
  stock-device path. See `A35-native-test-path.md`.
* **The `2627` testing-mode setup path on the A35 is `UNKNOWN`.** `CellBroadcastReceiver` line 195
  admits the secret code when `ro.debuggable=1` **or** `allow_testing_mode_on_user_build` is set.
  `allow_testing_mode_on_user_build` defaults to `true` in `config.xml`, but it is *not* listed in
  `res/values/overlayable.xml` — that file's single block is `CellBroadcastCustomization` — so this
  document does **not** claim Samsung can turn it on through a resource overlay. Whether Samsung
  ships an override, and whether the A35 is in testing mode, can only be settled by reading
  `dumpsys activity broadcasts` on the handset. No value is asserted here.
* **Samsung's package identity is `UNKNOWN`.** Samsung devices may provide Cell Broadcast from
  `com.samsung.android.cellbroadcastreceiver` rather than `com.google.android.cellbroadcastreceiver`.
  If it does, Samsung's variant may gate channels differently, and the catalogue above is AOSP. The
  controller already discovers the package by substring match on `cellbroadcast`, so it does not need
  the name in advance, but the catalogue's *gating* claim does not automatically transfer to a
  Samsung build. The read-only probe reports which package exists; this document does not guess.

---

## 5. Why this mattered enough to fix

The project's recorded bugs are almost all false successes. This one is the quieter inverse: a real
command that reaches a real receiver and is thrown away by a filter, on a channel the tool presented
as the only option. It would not have been found by testing an injected PDU on a `userdebug` device,
because there testing mode is usually on. It was found by reading what the phone does with the
message after it decodes it, which is the half of the system the tool used to treat as someone else's
problem.
