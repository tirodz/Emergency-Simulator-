# BUG-019: blocking audio preparation froze the alert UI

| Field | Value |
| --- | --- |
| Status | FIXED |
| Severity | MEDIUM |
| Affected component | `android/local-simulator/.../EmergencyActivity.kt` |
| Environment | Android 35 emulator; any device whose alarm tone is missing or slow to open |
| Discovered | First end-to-end run of the local simulator path against a real device |
| Commit | (this branch) |

## Summary

`startAudio` called `MediaPlayer.prepare()` on the main thread. When the resolved ringtone URI cannot
be opened, that call blocks until the platform gives up rather than failing immediately. On the
reference emulator it blocked for about **13 seconds**:

```
23:17:29.067  AUDIO_FOCUS_REQUEST granted=true
23:17:43.020  AUDIO_UNAVAILABLE IOException for content://settings/system/alarm_alert; ...
```

Thirteen seconds on the main thread during `onStart` is well past the point where Android considers an
application unresponsive. On a device with a slow or remote media source this is an ANR waiting to
happen — the alert would be killed by the system while trying to play its own warning tone.

The failure was also silent in the sense that mattered: the audio simply did not start, and the only
record of why was a stage line most runs would never read.

## Reproduction

1. Use a device image with no ringtone media (`/system/media/audio/` absent, `alarm_alert` null).
2. Send an alert and time `AUDIO_FOCUS_REQUEST` to `AUDIO_UNAVAILABLE` in logcat.
3. The gap is the main thread being blocked in `prepare()`.

## Fix

Preparation moved to `prepareAsync()` with an `OnPreparedListener`, so the alert is rendered and
interactive while the tone is being resolved. Three supporting changes were needed to make that
safe:

* A **candidate fallback chain** (alarm tone, then notification tone), because a URI that exists can
  still fail to open. The alert falls back to the notification sound instead of falling silent.
* A **generation counter** (`audioGeneration`), incremented in `stopEffects`. Async callbacks compare
  against it, so a player superseded by a dismissal or a repeated alert can never start sound after
  the alert is gone. Without this, async preparation would have introduced exactly the "sound keeps
  playing after dismissal" defect that the `onStart`/`onStop` bracketing exists to prevent.
* **`pendingPlayer` tracking**, so a player still preparing is released on teardown instead of
  leaking.

Verified after the change: `grep -ci "ANR in com.tirodz"` → 0, and the alert still renders
full-screen with vibration.

## What is *not* a defect here

`AUDIO_UNAVAILABLE` on the reference emulator is **correct reporting**, not a bug. That image ships
no ringtone media at all:

```
$ adb shell ls /system/media/audio/alarms/
ls: /system/media/audio/alarms/: No such file or directory
$ adb shell settings get system alarm_alert
null
```

The stage line is telling the truth about the device. Recording this here so a future session does not
"fix" a correct report.

## Related

* BUG-002/BUG-006 — earlier defects in the same lifecycle area, concerning accumulation and the
  alert surviving dismissal. The generation counter is the async-safe form of the same guarantee.
