# BUG-017: the evidence timeout was shorter than the platform's own alert latency

| Field | Value |
| --- | --- |
| Status | FIXED |
| Severity | HIGH |
| Affected component | `src-tauri/src/lib.rs`, `collect_local_simulator_evidence` |
| Environment | Android 35 emulator, local simulator path; any slow or loaded device |
| Discovered | First end-to-end run of the local simulator path against a real device |
| Commit | (this branch) |

## Summary

The collector polled logcat for 8 seconds and then reported
`LOCAL_UI_EVIDENCE_TIMEOUT`. On the device the measured latency from broadcast to a rendered
full-screen alert was **12.7 seconds**:

```
23:17:12.859  ANDROID_RECEIVER_ACCEPTED category=4355 severity=TEST chars=10
23:17:18.633  NOTIFICATION_POSTED id=4355
23:17:26.579  FULLSCREEN_ACTIVITY_STARTED fullScreenAllowed=true
23:17:27.655  FULLSCREEN_ACTIVITY_STARTED rendered
23:17:29.067  AUDIO_FOCUS_REQUEST granted=true
```

The alert appeared. The tool would have said it did not. This is a **false negative**, the mirror
image of the false successes already recorded in this directory, and it is arguably worse: a false
success tells the operator to trust something that is not there, while a false failure tells them to
distrust something that is.

The 8 s budget was not measured against anything. It was a plausible-looking round number, and it was
wrong by more than a factor of three on an unaccelerated emulator. A loaded physical device or a
cold-started app would be slower still.

## Reproduction

1. Install the local simulator on an Android 35 device and grant
   `POST_FULL_SCREEN_INTENT`/`USE_FULL_SCREEN_INTENT`.
2. Sleep the device so a full-screen intent is actually presented.
3. Send one alert and time broadcast-to-`FULLSCREEN_ACTIVITY_STARTED` in logcat.
4. On the reference emulator this is 12.7 s, which exceeds the old 8 s ceiling.

## Fix

`LOCAL_EVIDENCE_TIMEOUT` is now 25 s, chosen with headroom over the 12.7 s measurement rather than
picked for looking tidy. The constant carries the measurement in its doc comment so a future session
does not "tidy" it back down.

The poll also returns as soon as a terminal stage is seen, so the larger ceiling costs time only on a
device that is genuinely not responding. A slow device now produces a correct answer late instead of
a wrong answer early.

## Why the unit tests did not catch this

The defect is a deadline, not a decision. `local_simulator_verdict` was correct throughout: given the
full set of stage lines it returns `ALERT_DISPLAYED`. It was simply never given the full set, because
the loop stopped asking before the later lines existed. Testing the verdict function could not
expose that, which is why the fix separates the pure decision from the polling loop and gives each
its own test surface.

## Related

* BUG-018 — the same run exposed a second timing defect, the premature notification-only verdict.
* BUG-007 — an earlier timeout-treated-as-failure defect in the retired Python controller. The same
  class of error, in the opposite direction.
