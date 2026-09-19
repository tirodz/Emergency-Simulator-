# BUG-004: packaged build guessed its own resource paths

| Field | Value |
| --- | --- |
| Status | FIXED |
| Severity | MEDIUM |
| Affected component | `app/controller.py` (module-level path constants) |
| Environment | PyInstaller 6.22.3 onefile build, Windows and Linux |
| Discovered | While making the build self-contained |
| Commit | `7c4e857` |

## Summary

The packaged executable derived every resource path from `sys.executable`'s directory, on the
assumption that a released build would ship loose files beside the binary. A PyInstaller onefile
build does not: it unpacks into a temporary directory exposed as `sys._MEIPASS` and deletes it on
exit. The packaged build could therefore fail to find its own Android injector while behaving
correctly when run from the repository.

## Reproduction

Build with PyInstaller `--onefile` and run the executable with the injector bundled as a data file
rather than placed next to the binary. The controller reports the injector as missing.

## Expected

The build finds the injector it was shipped with, regardless of how PyInstaller chooses to unpack it.

## Actual

Under the original layout the path resolved relative to the executable's directory, which is correct
only when the jar is also placed there by hand. A genuine onefile release does not satisfy that.

## Root cause

The path logic encoded an assumption about the packaging mode rather than resolving it: files were
looked for in exactly one place, chosen by `sys.frozen`. That assumption happened to hold for the
manually-tested build, which is the worst kind of packaging bug -- it passes the test that was run
and fails the case that was not.

## Fix

`app/runtime.py` now resolves resources by searching, in order:

1. `sys._MEIPASS`, where PyInstaller unpacks bundled data;
2. the executable's own directory, for a loose-file or directory-mode build;
3. the repository root, for a development checkout.

The first location that actually contains the resource wins. The controller, the CLI and the UI all
ask this one module, so they cannot disagree about where the injector is.

## Regression test

`python3 tools/test_alert.py --adb-info` prints the resolved layout, the search roots actually used
and the resolved injector path. It is the diagnostic used to confirm a packaged build is
self-contained.

## Lesson

A build that must be self-contained should resolve what it actually has, not assume where it will be
put. The failure mode here was invisible because the development layout also worked.