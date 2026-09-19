# BUG-005: application fails with no usable adb, instead of explaining

| Field | Value |
| --- | --- |
| Status | FIXED |
| Severity | HIGH |
| Affected component | `app/runtime.py`, `app/adb.py`, `app/controller.py`, UI startup |
| Environment | Any machine without Platform Tools, or with a broken copy |
| Discovered | During release-hardening review |
| Commit | `7c4e857` |

## Summary

When adb was absent, the tool either produced a bare error or, in the desktop interface, never
reached a usable window. On Windows the common case is not "adb is missing" but "adb.exe is present
and cannot load": `adb.exe` needs `AdbWinApi.dll` and `AdbWinUsbApi.dll` beside it, and without them
it fails at load time with a generic loader error. That is indistinguishable from a missing file to
any check that only tests whether the path exists.

## Reproduction

```
python3 tools/test_alert.py --adb /nonexistent/adb --list
```

and, on Windows, placing `adb.exe` somewhere without its companion DLLs.

## Expected

A clear, actionable message naming what was found, what was wrong with it, and what to do. The
desktop interface should open and say so, not fail to start.

## Actual

The CLI printed a short error and the UI's behaviour depended on where the failure surfaced. A
present-but-unloadable adb was treated as usable because the file existed, so the first real failure
appeared later as an unrelated-looking error.

## Root cause

Discovery checked for the presence of a file. It never executed the executable, so a copied-without-
its-DLLs `adb.exe` passed discovery and failed only when first used, far from the cause.

## Fix

- `probe_adb()` runs `adb version` and records the version, or the specific reason it failed,
  including the exit code and the program's own output.
- `locate_adb()` returns a mode -- `BUNDLED`, `EXTERNAL` or `MISSING` -- and a problem string.
- A bundled copy that is present but unusable is reported as a packaging fault rather than being
  silently replaced by a system adb, so the defect stays visible.
- `adb_setup_hint()` gives operator-facing guidance that names no cellular involvement and points at
  the official Platform Tools page.
- `--adb-info` reports the whole picture on demand.

Example of the improved diagnostic:

```
ADB
  mode:    MISSING
  path:    /tmp/fakeadb
  usable:  no (exited 127: error while loading shared libraries)
```

## Regression test

`tools/test_alert.py --adb-info` reports the resolution for the current machine, and
`tools/test_controller.py` asserts that discovery does not report a non-executing adb as usable.

## Lesson

"Exists" is not "works", and on Windows those differ in a way that a file check cannot see. Anything
that is executed should be probed by executing it.