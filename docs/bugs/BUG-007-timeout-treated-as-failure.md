# BUG-007: a timeout was reported as a failure, implying nothing was delivered

| Field | Value |
| --- | --- |
| Status | FIXED |
| Severity | HIGH |
| Affected component | `app/controller.py`, `_collect_evidence`; `app/models.py`, `TransactionState` |
| Environment | Any; relevant whenever evidence collection exceeds `EVIDENCE_TIMEOUT` |
| Discovered | During release-hardening review |
| Commit | `7c4e857` |

## Summary

When the pipeline produced no alert evidence within the timeout, the result was reported as a plain
failure. That wording says nothing happened. It is not true: a timeout means the outcome is unknown.
The injector may have delivered the message and the device may be showing an alert right now. An
operator who reads `FAILED` and sends again will stack alerts.

## Reproduction

Send to a device and stop evidence collection early, or use a device slow enough that the alert UI
appears after `EVIDENCE_TIMEOUT` (45 seconds). The result is reported as `FAILED / TIMEOUT`.

## Expected

A timeout should be reported as an indeterminate outcome, and it should gate further sends until a
human has checked the device.

## Actual

`FAILED` with `TIMEOUT`. The device gate in BUG-006 either did not exist or did not distinguish this
case, so a follow-up send was permitted on a device that might already have been displaying an alert.

## Root cause

The evidence collector had only two outcomes: evidence found (success) and evidence not found
(failure). "Not found within the time available" is a third outcome and was collapsed into the
second. The distinction matters most in exactly the situation where it is hardest to observe.

## Fix

The gate gained `UNCERTAIN`, and `_collect_evidence` sets it for every indeterminate result:
a timeout with no evidence, an alert service that started but produced no UI, a receiver that saw the
message but did not start the service, and transport errors after injection. A subsequent send is
refused with an explanation that tells the operator to look at the device screen first:

```
the previous attempt to this device had an uncertain outcome. Check the device screen before
sending again: a timeout is not proof that the alert was not delivered.
```

Only two outcomes clear the gate to `READY` automatically: an explicit downstream rejection, and
cancellation before injection, because in both cases delivery is genuinely impossible.

## Regression test

`tools/test_controller.py` asserts that a timeout sets `UNCERTAIN` rather than `READY`, and that a
send to an `UNCERTAIN` device is refused with `DUPLICATE_SEND_BLOCKED`.

## Lesson

For a channel that cannot be observed and cannot be undone, the safe default for an unknown outcome
is uncertainty, not failure. The cost of one extra acknowledgement is far below the cost of a
duplicate emergency alert.