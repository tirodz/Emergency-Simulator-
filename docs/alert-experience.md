# The alert experience

What happens, mechanically, once a message has passed all the gates. This is the part of the pipeline
the project's whole value proposition rests on: it is Android's own code, not ours.

Source: `packages/apps/CellBroadcastReceiver`, `android16-release`.

## 1. Dispatch

`CellBroadcastAlertService.openEmergencyAlertNotification(message)` is the single decision point. It:

1. chooses an alert type (`AlertType`: `DEFAULT`, `ETWS_DEFAULT`, `ETWS_EARTHQUAKE`, `ETWS_TSUNAMI`,
   `TEST`, `AREA`, `INFO`, `MUTE`, `OTHER`);
2. launches `CellBroadcastAlertAudio` with the chosen tone and DND/inbox options;
3. launches `CellBroadcastAlertDialog` for emergency alerts;
4. posts the notification;
5. writes to the history database via `CellBroadcastContentProvider`.

The alert type selection:

* `message.isEtwsMessage()` -> the `SmsCbEtwsInfo` warning type selects the tone
  (`etws_earthquake.ogg`, `etws_tsunami.ogg`, `etws_other_disaster.ogg`, `etws_default.ogg`);
* otherwise -> the channel range's `AlertType` selects the tone (`TEST` / `DEFAULT` -> `default_tone.ogg`,
  `AREA` -> `area.ogg`, watch-info -> `watch_info.ogg`).

## 2. Sound

`CellBroadcastAlertAudio` (`CellBroadcastAlertAudio.java`):

```java
AudioAttributes.Builder builder = new AudioAttributes.Builder();
builder.setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION);
builder.setUsage(mAlertType == AlertType.TEST
        ? AudioAttributes.USAGE_NOTIFICATION : AudioAttributes.USAGE_ALARM);
if (mOverrideDnd) {
    // only works when usage is USAGE_ALARM
    builder.setFlags(AudioAttributes.FLAG_BYPASS_INTERRUPTION_POLICY
            | AudioAttributes.FLAG_BYPASS_MUTE);
}
```

and volume:

```java
// overwrite volume setting of STREAM_ALARM to full
mAudioManager.setStreamVolume(AudioManager.STREAM_ALARM, maxVolume, 0);
```

Consequences worth stating plainly:

* A **test-class** alert uses `USAGE_NOTIFICATION`, **not** `USAGE_ALARM`. It is quieter in intent and
  does **not** get the DND-bypass flags.
* Only `USAGE_ALARM` accepts `FLAG_BYPASS_INTERRUPTION_POLICY` / `FLAG_BYPASS_MUTE`; the code comment
  says so explicitly.
* An alarm-class alert forces the alarm stream to maximum for its duration and then restores the user's
  volume (`mUserSetAlarmVolume`).
* The alert also speaks the body via TTS (`mTts`, subject to settings and to `getAlertAudioAttributes()`).

This is a key design tension for the project: **the categories that are safest (test categories) are
also the ones Android deliberately makes less aggressive.** A dramatic demo and an honest test
category pull in opposite directions. The project's position is stated in
[`security-and-safety.md`](security-and-safety.md): prefer the honest category and accept the milder
presentation.

## 3. Vibration

Same service:

```java
VibrationEffect effect = VibrationEffect.createWaveform(patternArray, -1);
...
AudioAttributes attr = attrBuilder.build();          // USAGE_ALARM when not a test alert
mVibrator.vibrate(effect, attr);
```

The vibration pattern comes from the channel range (`vibration=` key) or from
`default_vibration_pattern` / `default_notification_vibration_pattern` (the latter specifically for
`AlertType.INFO`).

**Open question:** whether a device in silent/vibrate-only mode still vibrates and still plays. The
source has an explicit ringer-mode branch ("Ringer mode: vibrate"). Behaviour is
`UNKNOWN — requires experimental verification`. -> Experiment 11.

## 4. Full-screen UI

`CellBroadcastAlertDialog` (`CellBroadcastAlertDialog.java`) sets:

```java
win.addFlags(WindowManager.LayoutParams.FLAG_FULLSCREEN
        | WindowManager.LayoutParams.FLAG_SHOW_WHEN_LOCKED
        | ...);
...
getWindow().addFlags(WindowManager.LayoutParams.FLAG_TURN_SCREEN_ON
        | WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
```

Details:

| Behaviour | Implementation | Notes |
| --- | --- | --- |
| Shows over lock screen | `FLAG_SHOW_WHEN_LOCKED` | applies to the real keyguard |
| Turns the screen on | `FLAG_TURN_SCREEN_ON` | cleared on screen-off (`onScreenOff`) |
| Keeps the screen on | `FLAG_KEEP_SCREEN_ON` | duration from the channel's `screen_on_duration`, default `KEEP_SCREEN_ON_DURATION_MSEC = 60000`; `0` means "do not turn screen on" |
| Secure content | `FLAG_SECURE` under some conditions | prevents screenshots of the alert |
| Dismiss button | `findViewById(R.id.dismissButton)` -> `dismiss()` | multiple messages show `1/N` |
| Dismiss on outside touch | channel key `dismiss_on_outside_touch` | |
| Warning icon | channel keys `display_icon`, and a pulsation pattern | |

## 5. Notification

A notification is posted alongside the dialog for emergency alerts. The CBR app holds
`android.permission.STATUS_BAR` and `BROADCAST_CLOSE_SYSTEM_DIALOGS` (per its privapp allowlist), which
is how it can present a system-modality alert and close the shade first.

Read messages can be removed from the notification bar (`removeReadMessageFromNotificationBar`), and
`DISMISS_NOTIFICATION_EXTRA` / `dismiss_notification` is used when an intent is only meant to dismiss.

## 6. Do Not Disturb

DND interaction is controlled by:

* the channel range's `override_dnd` key,
* the global `override_dnd` setting (the `SETTING_OVERRIDE_DND` / global value read at service start),
* and, for audio, `AudioAttributes.FLAG_BYPASS_INTERRUPTION_POLICY`, which is only effective for
  `USAGE_ALARM`.

So: **alarm-class alerts can bypass DND; test-class alerts generally cannot.** This is another case
where the honest test category produces a less dramatic result.

## 7. Dismissal

`CellBroadcastAlertDialog.dismiss()`:

* stops the `CellBroadcastAlertAudio` service (sound, vibration, TTS),
* stops the pulsation animation,
* cancels any pending alert reminder,
* removes the message from the notification bar.

The service also recognises an internal `DISMISS_DIALOG` intent and
`dismissAllFromNotification(intent)`.

**All of these are internal to the privileged CBR app.** No exported entry point has been found that
would allow a third-party component (or an ADB command) to dismiss a displayed alert. This is
documented as a security boundary, not as a bug. -> Experiment 9.

## 8. Duplicate and repeated alerts

* `CellBroadcastContentProvider.insertNewBroadcast(message)` returns false for a message already in the
  database. A duplicate insert therefore does **not** produce a second alert.
* Duplicate detection is keyed on the message identity (serial number + service category + format +
  location).
* Practical consequence for the tool: reuse of a serial number makes the second alert a no-op. The
  controller must manage serial numbers deliberately.
* There is also a reminder mechanism (`CellBroadcastAlertReminder`) that re-alerts for unacknowledged
  messages; `cancelAlertReminder()` stops it. Whether this produces a repeated sound is
  `UNKNOWN — requires experimental verification`.

## 9. Alert history

Written by `CellBroadcastContentProvider` into `Telephony.CellBroadcasts`. The history is user-visible
in the CB settings UI. `read` state is tracked and used to clear notifications. This is the most
convenient non-invasive way to *prove* that a test alert reached the genuine pipeline: the row appears
in Android's own history.

## 10. What is OEM-dependent

| Aspect | AOSP | OEM risk |
| --- | --- | --- |
| Titles / wording | `res/values/strings.xml` | overlayed |
| Channel ranges | `res/values/config.xml` + MCC/MNC overlays | overlayed |
| Whether the dialog is used at all | yes | a forked receiver may present differently |
| Whether SystemUI (Samsung) intercepts | no | **UNKNOWN** |
| Settings screen layout | AOSP | overlayed |

The *mechanism* is AOSP; the *presentation* is not. See [`oem-compatibility.md`](oem-compatibility.md).
