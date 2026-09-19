# BUG-009: a result banner could overwrite the permanent safety statement

| Field | Value |
| --- | --- |
| Status | FIXED |
| Severity | HIGH |
| Affected component | `app/ui.py`, outcome rendering |
| Environment | Desktop interface, any platform |
| Discovered | During the interface rebuild, by tracing what each render path wrote |
| Commit | `1fed923` |

## Summary

The interface had a single banner at the top of the window, and the result renderers wrote into it.
A successful alert replaced the permanent safety statement with a success message. A failure replaced
it with a failure message.

The result looked fine in both cases, which is why it survived: a success message where the safety
statement used to be is not obviously wrong on screen.

## Reproduction

Build the interface, then render any result:

```python
ui._render_result(SendResult(state=AlertState.ALERT_DISPLAYED, device_serial="emulator-5554"))
print(ui.safety_banner._label.cget("text"))
```

## Expected

The safety statement is shown whenever the window is open. It is the one statement that must always
be true, and it is the reason the window cannot be mistaken for a real alert.

## Actual

```
GENUINE ALERT DISPLAYED ON emulator-5554 -- DISMISS IT ON THE DEVICE
```

The statement `TEST ONLY / CONTROLLED DEVICE / NO CELLULAR TRANSMISSION` was gone. The operator now
had no visible indication that this was a test, on a window that had just displayed a message about a
genuine emergency alert.

## Root cause

One widget served two purposes with different lifetimes. The safety statement is permanent
state; results are transient events. Rendering an event over permanent state destroys the permanent
state, and nothing restores it afterwards.

## Fix

Two separate strips:

* `safety_banner` is created once and never written to again.
* `outcome_banner` is shown below it on demand, and holds every result message.

Two tests assert it directly:

* a displayed alert writes the outcome strip and leaves the safety strip intact
* a failure writes the outcome strip and leaves the safety strip intact

## Why this matters

This is a case where the visual defect and the safety defect are the same defect. The strip exists to
make the simulation unmistakable, and its value depends entirely on being always present. A banner
that is correct most of the time is not a weaker version of that guarantee; it is the absence of it.

The general rule, recorded so it is applied elsewhere: permanent safety information must live in a
widget that no event path is allowed to write to, and that separation should be enforced by a test
rather than by convention.