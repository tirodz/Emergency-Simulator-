# BUG-002: Cell Broadcast secret code treated as a setter but is a toggle

| Field | Value |
| --- | --- |
| Status | FIXED |
| Severity | HIGH |
| Affected component | `app/controller.py`, `prepare_test_mode` |
| Environment | Android 15 / API 35, `sdk_gphone64_x86_64`, userdebug |
| Discovered | During test-mode preparation work (see `docs/aosp-test-path.md` §6) |
| Commit | `ec40f49` |

## Summary

The controller used the dialer secret code `*#*#2627#*#*` to "enable" Cell Broadcast testing mode.
That code does not enable anything: it flips the current state. Sending it to a device that was
already in testing mode turned testing mode **off**, and the subsequent send was silently dropped.

## Reproduction

With testing mode already enabled on the device:

```
adb shell am broadcast \
  -a android.telephony.action.SECRET_CODE \
  -d "android_secret_code://2627"
```

Testing mode is now disabled. A test alert sent afterwards does not produce an alert.

## Expected

After preparation, testing mode is enabled, so the receiving app processes the test alert.

## Actual

On a device that was already in the desired state, preparation toggled it back off. The alert was
dropped. Depending on the device's preference defaults this produced either a `TEST_MODE_DISABLED`
failure or, worse, a run where the message reached the receiver and was filtered by user preference.

## Raw output

The receiver logs the drop, from `CellBroadcastReceiver`:

```
ignoring alert of type ... by user preference
```

and, from the framework, for the broadcast itself:

```
Broadcast completed: result=0, delivered=1, finished=1
```

Neither line is an error. `result=0` is what a successful broadcast of a *toggle* looks like.

## Root cause

`am broadcast` reports whether the broadcast was delivered, not what the receiver did about it. The
secret-code handler in AOSP is a toggle:

```java
// CellBroadcastReceiver
mTestingMode = !mTestingMode;
```

so the command is not idempotent. A controller that issues it on every run will disable the feature
about half the time, and the failure is invisible because the broadcast still reports success.

## Fix

Preparation reads the receiver's own preference file first and only acts when a change is needed:

- `app/controller.py` `read_test_mode()` parses
  `/data/user_de/0/<pkg>/shared_prefs/<pkg>_preferences.xml` for the `testing_mode` and
  `enable_test_alerts` flags.
- `prepare_test_mode()` sends the toggle **only** when reading showed it off.
- `_write_prefs()` sets `enable_test_alerts` directly, because that is a plain preference and has no
  toggle-code equivalent.

Verified: a device already in the correct state is left untouched, which is visible in the log as
`Test mode already enabled; leaving device configuration untouched`.

## Regression test

`tools/test_controller.py` asserts that `prepare_test_mode` performs no configuration change when the
device already reports the required flags, and that it reads state before writing.

## Lesson

Idempotence is not a nicety for a control channel that flips device state. The AOSP secret code is
designed for a human with a dialer, who can see what happened; a controller must read state, act only
on a difference, and confirm the result.

## Correction (2026-09-23): the reproduction could not have produced that observation

Re-reading `frameworks/base/core/res/AndroidManifest.xml` on `android14-release` this session shows
`android.telephony.action.SECRET_CODE` at line 564 in the platform's `<protected-broadcast>` list:

```xml
<protected-broadcast android:name="android.telephony.action.SECRET_CODE" />
```

`ActivityManagerService` refuses a protected broadcast from any caller that is not a system UID
(`ROOT_UID`, `SYSTEM_UID`, `PHONE_UID`, `BLUETOOTH_UID`, `NFC_UID`, `SE_UID`, `NETWORK_STACK_UID`),
and `SHELL_UID = 2000` is not among them. So:

```
adb shell am broadcast -a android.telephony.action.SECRET_CODE -d "android_secret_code://2627"
```

is refused with a `SecurityException` before any receiver runs. It cannot have toggled testing mode
in the direction the reproduction describes, and the `Broadcast completed: result=0, delivered=1,
finished=1` line quoted above cannot come from this command's delivery to `CellBroadcastReceiver`.

What survives from this bug:

* The handler really is a non-idempotent toggle — `setTestingMode(!isTestingMode(mContext))`, read in
  `CellBroadcastReceiver.java` lines 194–212 — so the wrong thing to do is issue it blindly.
* Reading state first and acting only on a difference remains the correct fix, and the regression
  test still stands.
* The secret code is **not** an injection route at all: it opens a display filter for test-mode
  channel ranges, consulted downstream in `CellBroadcastAlertService`. See
  `docs/stock-device/A35-native-test-path.md` §4.

The lesson this adds is the project's own: **an accepted command is not an observed effect, and a
bug report is not evidence.** This document asserted a reproduction that the platform refuses. It was
carried for several sessions without anyone re-running it against the source, which is exactly the
false-success pattern `docs/bugs/` exists to catch — committed here by the bug journal itself.
