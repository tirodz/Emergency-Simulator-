# BUG-018: the verdict was fixed at the notification, before the full-screen activity arrived

| Field | Value |
| --- | --- |
| Status | FIXED |
| Severity | MEDIUM |
| Affected component | `src-tauri/src/lib.rs`, `collect_local_simulator_evidence` |
| Environment | Android 35 emulator, local simulator path; any device |
| Discovered | First end-to-end run of the local simulator path against a real device |
| Commit | (this branch) |

## Summary

`NOTIFICATION_POSTED` was treated as conclusive. The collector saw it, set
`result.state = "NOTIFICATION_POSTED"` and returned.

Android does not post the notification and launch the full-screen activity together. It posts the
notification first and presents the full-screen intent afterwards, **8 seconds apart** in the
captured run:

```
23:17:18.633  NOTIFICATION_POSTED id=4355
23:17:26.579  FULLSCREEN_ACTIVITY_STARTED fullScreenAllowed=true
```

So the collector returned a notification-only verdict for an alert that did take over the screen. The
operator would be told "the alert did not take over the screen" about an alert that had.

This is not the dangerous direction of error — it understates rather than overstates — but it is the
same underlying mistake the project exists to catch: a conclusion drawn from an incomplete set of
signals because the moment of decision was chosen for convenience rather than from evidence.

## Reproduction

1. Sleep the device and send an alert.
2. Read `NOTIFICATION_POSTED` and `FULLSCREEN_ACTIVITY_STARTED` timestamps from logcat.
3. They are seconds apart, not simultaneous.

## Fix

After the first conclusive verdict that is *not* already full-screen, the collector keeps polling for
a bounded grace window (`FULLSCREEN_GRACE = 14 s`) so a full-screen stage can still land before the
verdict is fixed. A failure, or an alert already confirmed full-screen, remains immediately final —
only the notification-only case waits, because that is the only case a later stage can upgrade.

The window is bounded rather than open-ended so the common stock-device outcome (notification posted,
full-screen withheld) still reports promptly instead of always waiting out the full timeout.

## Note on the interaction with BUG-017

These two defects were pulling in opposite directions and both were real. Raising the timeout alone
would not have fixed this one: the collector was not running out of time, it was concluding early. A
short timeout and an eager verdict are different bugs that happened to look similar from the outside
— both produced a notification-only answer for a full-screen alert.

## Related

* BUG-017 — the companion timing defect found in the same run.
