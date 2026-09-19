# Bug journal

Every meaningful defect found in this project is recorded here, whether or not it was fixed. The
point is durable evidence: a future maintainer (or a coordinating agent working from GitHub) should
be able to read a record, understand exactly what went wrong and why, reproduce it, and see which
test now prevents it from coming back.

## Why this exists

One of the earliest defects in this project was dangerous precisely because it looked like success. A
multi-word alert body was silently truncated by the transport, the injector exited 0, and the genuine
alert appeared; nothing anywhere reported an error. The only trace was a truncated row in the
device's own history database. A project that patches such a thing quietly and moves on will ship it
again.

So: no silent patches.

## What each record contains

| Field | Meaning |
| --- | --- |
| Title | Short, specific summary |
| Status | OPEN, FIXED, WONTFIX, or UNKNOWN |
| Severity | How much it damages trust in the tool |
| Affected component | The file or subsystem at fault |
| Environment | Device, OS, versions |
| Reproduction | Exact steps and commands |
| Expected | What should happen |
| Actual | What did happen |
| Raw output | Verbatim logs or command output |
| Root cause | The mechanism, or `ROOT CAUSE: UNKNOWN` if not established |
| Fix | What changed |
| Regression test | The test that now guards it |
| Commit | The commit containing the fix |

## Index

| ID | Title | Severity | Status |
| --- | --- | --- | --- |
| [BUG-001](BUG-001-adb-shell-truncates-alert-body.md) | `adb shell` silently truncates a multi-word alert body | HIGH | FIXED |
| [BUG-002](BUG-002-secret-code-toggle.md) | Cell Broadcast secret code treated as a setter but is a toggle | HIGH | FIXED |
| [BUG-003](BUG-003-prefs-read-failure-looks-disabled.md) | A preference read failure looked like "disabled" | MEDIUM | FIXED |
| [BUG-004](BUG-004-frozen-paths-guessed.md) | Packaged build guessed its own resource paths | MEDIUM | FIXED |
| [BUG-005](BUG-005-no-adb-fails-without-explanation.md) | Application fails with no usable adb, instead of explaining | HIGH | FIXED |
| [BUG-006](BUG-006-duplicate-alert-accumulation.md) | Repeated sends stack emergency dialogs on the device | HIGH | FIXED |
| [BUG-007](BUG-007-timeout-treated-as-failure.md) | A timeout was reported as a failure, implying nothing was delivered | HIGH | FIXED |
| [BUG-008](BUG-008-default-log-dir-namerror.md) | `default_log_dir()` referenced a removed constant | MEDIUM | FIXED |
| [BUG-009](BUG-009-safety-strip-overwritten.md) | A result banner could overwrite the permanent safety statement | HIGH | FIXED |
| [BUG-010](BUG-010-selftest-race.md) | The self-test verification read its report before the executable wrote it | MEDIUM | FIXED |

## Severity scale

| Level | Meaning |
| --- | --- |
| CRITICAL | Causes a real-world emergency alert, or a false success that could mislead about one |
| HIGH | Produces a wrong result, a false success, or blocks a supported device entirely |
| MEDIUM | Degrades reliability or diagnostics; workaround exists |
| LOW | Cosmetic or documentation |
| [BUG-011](BUG-011-ui-accent-token-missing.md) | Polished UI referenced an undefined accent token | HIGH | FIXED |
| [BUG-012](BUG-012-ui-devicestate-import-missing.md) | Polished device renderer referenced an undefined DeviceState | HIGH | FIXED |
| [BUG-013](BUG-013-ui-tests-hardcoded-theme-colors.md) | UI regression tests kept stale hardcoded palette values after theme refresh | MEDIUM | FIXED |
| [BUG-014](BUG-014-installer-artifact-path-mismatch.md) | Installer job looked for the executable at the wrong artifact path | HIGH | FIXED |
