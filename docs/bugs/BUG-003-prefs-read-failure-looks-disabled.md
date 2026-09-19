# BUG-003: a preference read failure looked like "disabled"

| Field | Value |
| --- | --- |
| Status | FIXED |
| Severity | MEDIUM |
| Affected component | `app/controller.py`, `read_test_mode` / `prepare_test_mode` |
| Environment | Android 15 / API 35, `sdk_gphone64_x86_64`, userdebug |
| Discovered | During test-mode preparation work |
| Commit | `ec40f49` |

## Summary

When the receiver's preference file could not be read, the controller treated the absent result as
"the flags are off". On a device where root had not yet been established, or where the path did not
exist yet, that produced a confusing failure: the tool would report that test alerts were disabled
and could not be enabled, even though it had never successfully read the state.

## Reproduction

Run a send against a device whose `adb root` has not succeeded, so the preference file cannot be
read. The failure reported is `TEST_MODE_DISABLED` rather than a privilege problem.

## Expected

An inability to read device state should be reported as exactly that, distinct from a confirmed
"disabled" state, so the operator knows whether to fix a privilege problem or change a setting.

## Actual

Both cases collapsed into the same `TEST_MODE_DISABLED` message and the same downstream behaviour.

## Root cause

`read_test_mode()` returned a default-constructed `TestModeStatus` for both outcomes, so a missing
file, a permission denial and a genuinely-disabled feature were indistinguishable. The caller had no
way to tell them apart.

## Fix

`TestModeStatus` carries the reason in its `error` field, and `read_test_mode()` sets it for each
distinct failure (no root, file not present, unparsable content). `prepare_test_mode` distinguishes a
read failure from a confirmed-off flag and reports the privilege or path problem rather than a
setting problem.

## Regression test

`tools/test_controller.py` asserts that an unreadable preference file yields an `error` rather than a
silent default, and that the resulting failure code is not `TEST_MODE_DISABLED`.

## Lesson

"Absent" and "false" are different facts. Collapsing them produces diagnostics that point at the
wrong subsystem, which for this tool means an operator debugging device settings when the real
problem is privilege.