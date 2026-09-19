# BUG-001: `adb shell` silently truncates a multi-word alert body

| Field | Value |
| --- | --- |
| Status | FIXED |
| Severity | HIGH |
| Affected component | `app/adb.py`, `Adb.shell` |
| Environment | Android 15 / API 35, `sdk_gphone64_x86_64`, userdebug |
| Discovered | During end-to-end controller testing (EXP-CTL-003) |
| Commit | `ec40f49` |

## Summary

Sending a message body containing spaces resulted in only the first word reaching the device. The
injector exited 0, the genuine emergency alert was displayed, and no layer reported an error. The
result was reported as `ALERT_DISPLAYED` while the device showed the wrong text.

## Reproduction

```
python3 tools/test_alert.py --device emulator-5554 --message "TEST ALERT - SIMULATION" --yes
```

The device's Cell Broadcast history database then contains the row:

```
4|4355|1|TEST
```

## Expected

The full body `TEST ALERT - SIMULATION` reaches the injector and appears in the alert and in the
history row.

## Actual

Only `TEST` reached the injector. Injector output showed `body = TEST`; exit code 0. The alert was
displayed, so the outcome looked like a complete success.

## Raw output

```
injector: body = TEST
Result: ALERT_DISPLAYED
```

and, from the device's history database:

```
4|4355|1|TEST
```

## Root cause

`adb shell` does not pass an argument vector. It joins its arguments into a single string which the
**device's** shell then re-parses. The body `TEST ALERT - SIMULATION` therefore arrived at the device
as three separate shell words, and only the first was bound to the injector's `argv[2]`.

Because the transport did not fail -- the command ran, and it ran successfully -- nothing reported a
problem. Exit codes cannot detect this class of defect, which is the reason this project verifies
outcomes against the device's own log evidence rather than trusting the injector's exit status.

## Fix

Every argument passed to `Adb.shell` is now quoted with `shlex.quote` before the arguments are
joined. The device shell sees exactly one word per argument, so nothing we send can be split or
reinterpreted. This also closes the command-injection surface, which mattered because the alert body
is operator-supplied.

Verified afterwards:

```
injector: body = TEST ALERT - SIMULATION
```

History row:

```
5|4355|1|TEST ALERT - SIMULATION
```

## Regression test

`tools/test_controller.py` exercises the transport with a multi-word body and asserts that the body
survives intact. `tools/test_alert.py` additionally validates the body before any device is touched.

## Why this record matters

This is the canonical example of a false success in this project. It is preserved because the
temptation to trust a zero exit code is strong, and this bug demonstrates concretely why we do not.