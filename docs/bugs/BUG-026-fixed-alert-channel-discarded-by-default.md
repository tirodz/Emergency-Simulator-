# BUG-026 — the controller always emitted the one channel a default phone discards

**Status:** FIXED
**Class:** correct command, discarded by a receive-side filter the tool never modelled
**Found:** 2026-09-21, platform-diagnostics session
**Files:** `src-tauri/src/platform.rs`, `src-tauri/src/lib.rs`, `src/index.html`

## What the code did

`send_platform_test_alert` built every Cell Broadcast PDU with a single hardcoded message identifier:

```rust
pub fn etws_test_pdu(body: &str, serial_number: u16) -> Result<Vec<u8>, String> {
    ...
    pdu.extend_from_slice(&MESSAGE_ID_ETWS_TEST.to_be_bytes()); // 0x1103
```

`MESSAGE_ID_ETWS_TEST` is `0x1103`. The interface presented this as settled fact, in a metric that
read `4355 / 0x1103 · locked`, and the confirmation dialog repeated it. There was no way to select a
different channel.

## Why it looked right

`0x1103` is the identifier in AOSP's own documented test command, quoted in `GsmInboundSmsHandler`:

```
adb shell am broadcast -a com.android.internal.telephony.gsm.TEST_TRIGGER_CELL_BROADCAST \
  --es pdu_string 0000110011010D0A... --ei phone_id 0
```

The third and fourth octets of that reference PDU are `11 03`. Pinning the channel to the reference
value is the obvious, and wrong, conclusion: the reference demonstrates the *wire format* of a test,
not the *receive configuration* needed for an alert to be raised.

## What actually happens

`CellBroadcastAlertService.isChannelEnabled` (`packages/apps/CellBroadcastReceiver`,
`android14-release`, line 497) resolves the message's service category to a range array and consults
a different preference per array:

| Channel | Array (line) | Preference | Default |
|---|---|---|---|
| `0x1100` ETWS primary | `etws_alerts_range_strings` (527) | master toggle only | on |
| `0x1113` CMAS extreme | `cmas_alert_extreme_channels_range_strings` (567) | master + CMAS extreme | on |
| `0x111C` monthly test | `required_monthly_test_range_strings` (592) | master + test alerts | off |
| `0x1103` ETWS test | `etws_test_alerts_range_strings` (520) | master + test alerts | off |

Defaults are `master_toggle_enabled_default=true` and `test_alerts_enabled_default=false` in
`res/values/config.xml`. `0x1103` additionally carries `testing_mode` in its range declaration, and
line 320 filters it out unless testing mode is on.

So on a device nobody configured, the message is decoded, handed to the alert service, and discarded.
The observable result is exactly the project's recurring failure shape: the PDU is well-formed, `am`
exits 0, the receiver fires, nothing is displayed, and no layer reports an error.

## Expected

A channel that produces an alert on a default device, or an explicit statement of what the operator
must enable.

## Actual

A fixed channel that requires testing mode *and* the test-alerts toggle, neither of which the tool
mentioned.

## Root cause

The send path was modelled from the AOSP *injection* entry point. The receive path — what the phone
does with a decoded message — was treated as downstream and out of scope. Both halves were in the
same repository the whole time.

## Fix

`ALERT_CHANNELS` in `platform.rs` is now a catalogue; each entry states its identifier, its gate, its
default state and the operator action required. `cb_pdu(message_id, body, serial)` refuses any
identifier not in the catalogue. The default became `0x1100`, which is on by default. The frontend
loads the catalogue through `list_alert_channels` and states the gate for the selected channel in the
confirmation dialog, so the panel cannot drift from the encoder.

## Regression tests

* `an_uncatalogued_channel_cannot_be_encoded` — a PDU cannot be built for an unvetted channel.
* `the_default_channel_needs_no_operator_action` — the default is on by default and is not `0x1103`.
* `the_catalogue_records_the_as_p_is_channel_enabled_gates` — the four per-channel gates are pinned.
* `the_requirement_text_matches_the_default_state` — a channel that is on by default does not invent
  a prerequisite.
* `the_refactor_preserved_the_etws_test_encoding` — `0x1103` output is byte-identical to before.
* `a_non_default_channel_encodes_into_the_header` — every catalogued channel lands correctly and its
  body round-trips.

## Scope note

This does **not** create a stock-device path. `0x1100` is reachable only through the telephony test
receiver, which is gated on `ro.debuggable=1`. The full reasoning, and the boundary between the
`CONFIRMED` AOSP gating model and the `UNKNOWN` state of the A35's own configuration, is in
`docs/stock-device/alert-channel-gating.md`.
