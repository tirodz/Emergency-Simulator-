# BUG-010: the self-test verification read its report before the executable wrote it

| Field | Value |
| --- | --- |
| Status | FIXED |
| Severity | MEDIUM |
| Affected component | `packaging/build_windows.ps1`, `.github/workflows/build-windows.yml` |
| Environment | Windows, PowerShell 7, a windowed PyInstaller build |
| Discovered | By CI on `windows-latest`, run 35449029117 |
| Commit | fixed immediately after that run |

## Summary

The build verification for the frozen executable never checked anything. It invoked the executable,
then immediately read the report file — before the executable had written it.

CI failed with:

```
Get-Content: Cannot find path 'C:\Users\RUNNER~1\AppData\Local\Temp\selftest-report.txt' because it does not exist.
self-test report written to C:\Users\RUNNER~1\AppData\Local\Temp\selftest-report.txt
##[error]Process completed with exit code 1.
```

The ordering of those two lines is the whole bug: the check failed, and *then* the report appeared.

## Root cause

A PyInstaller `--windowed` build produces a **GUI-subsystem** executable (`/SUBSYSTEM:WINDOWS`, no
console). When PowerShell invokes such a binary directly:

```powershell
& $exe "--selftest=$report"
$text = Get-Content $report -Raw     # too early
```

the call returns immediately without waiting for the process. This is why `$LASTEXITCODE` was also
unreliable here — the process had not exited, so there was no exit code to read. The verification was
therefore a no-op that always failed, or worse, would have passed against a stale report.

`Start-Process -Wait -PassThru` waits for a GUI-subsystem process properly and gives a real
`ExitCode`.

## Fix

```powershell
Remove-Item $Report -ErrorAction SilentlyContinue
$Proc = Start-Process -FilePath $Exe -ArgumentList "--selftest=$Report" -Wait -PassThru
$SelfTestCode = $Proc.ExitCode
if (-not (Test-Path $Report)) { throw "The self-test wrote no report to $Report (exit $SelfTestCode)." }
```

The stale report is deleted first, so a previous run's success can never be mistaken for this one's.

## Why this matters

This is the third defect in this project of the same shape: **a check that cannot fail for the right
reason is not a check.** BUG-007 reported a timeout as a failure. BUG-008 made a helper crash on a
missing constant. This one reported success or failure based on a race with a file.

It is worth noting that the verification was written specifically to catch BUG-004 (a build that
cannot find its own resources) and was itself broken from the moment it was written. The test was on
Linux, where the binary is a console executable that *does* block, so it passed locally and failed
only on Windows — the one platform it existed for.

Recorded so the next person to write a verification step asks two questions: does this actually
observe the thing it claims to, and would it fail if the thing were broken?

## Regression coverage

CI now runs this verification on `windows-latest` against the real artifact, so the failure mode is
covered by definition. There is no unit test: the defect is in the harness, and the real executable
on the real platform is the only thing that can exercise it.