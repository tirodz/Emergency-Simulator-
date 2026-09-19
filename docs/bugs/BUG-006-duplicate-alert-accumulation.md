# BUG-006: repeated sends stack emergency dialogs on the device

| Field | Value |
| --- | --- |
| Status | FIXED |
| Severity | HIGH |
| Affected component | `app/controller.py`, send path; `app/models.py`, `TransactionState` |
| Environment | Android 15 / API 35, `sdk_gphone64_x86_64`, userdebug |
| Discovered | During release-hardening review |
| Commit | `7c4e857` |

## Summary

Nothing prevented sending a second test alert while the first was still displayed. Android queues
emergency alerts, and a displayed alert cannot be withdrawn by any application, so pressing SEND
again does not retry a failure -- it leaves the device showing additional stacked dialogs that the
operator must dismiss one by one, each with sound.

## Reproduction

Send two alerts in succession without dismissing the first:

```
python3 tools/test_alert.py --device emulator-5554 --yes
python3 tools/test_alert.py --device emulator-5554 --yes
```

## Expected

The second attempt should be refused while the previous alert is still outstanding on the device,
with an explanation and a way to clear the condition once the operator has dismissed it.

## Actual

Before the fix the second send proceeded. The device displayed a further alert, and the history
database recorded a further row. Because Android queues rather than replaces, the operator had to
dismiss each one.

## Raw output

After the fix, the second send is refused before the device is touched at all:

```
refusing to send: the previous test alert is still outstanding on this device. Android queues
alerts, so sending again would stack a second dialog. Dismiss the alert on the device, then
acknowledge it here.
```

and the sequence reports:

```
send result : ALERT_DISPLAYED
gate after  : DELIVERED
SECOND SEND : FAILED | DUPLICATE_SEND_BLOCKED
```

## Root cause

The controller treated each send as an independent operation. It had no model of the device's
outstanding state, and Android offers no way to ask "is an emergency alert currently displayed?" --
the alert dialog is not queryable by a third party, and closing it is deliberately blocked. Since the
state cannot be observed, it has to be remembered.

## Fix

A per-device gate with four states:

| State | Meaning | A send is |
| --- | --- | --- |
| `READY` | No outstanding alert | allowed |
| `BUSY` | A send is in progress | refused |
| `DELIVERED` | An alert was displayed and not yet acknowledged | refused |
| `UNCERTAIN` | Outcome unknown (timeout, transport error) | refused |

The gate closes when injection can first have reached the device and clears on
`acknowledge(serial)`, which the operator calls once they have dismissed the alert on the device.
`send_test_alert` checks the gate before anything else, and a refusal sets
`DUPLICATE_SEND_BLOCKED`. The CLI exposes `--acknowledge`; the UI exposes an acknowledge control.

## Regression test

The end-to-end test above, reproduced on `emulator-5554`: the first send reaches `ALERT_DISPLAYED`
and sets `DELIVERED`, the second is refused with `DUPLICATE_SEND_BLOCKED`, and `acknowledge` returns
the device to `READY`.

## Lesson

When a state cannot be read, it must be tracked, and clearing it must be an explicit human act. The
controller must not decide on the operator's behalf that an alert has been dismissed, because it has
no way to know whether it has.