# BUG-025 — the paste's did-anything-change check never ran

**Status:** FIXED
**Class:** false success — a verification step that could not fail
**Found:** 2026-09-22, overnight investigation session
**File:** `docs/connect-one-paste.md`

## What the code did

Step 5 of the operator paste exists to answer the question the whole project is organised around:
*did this touch the phone?* It read a variable to decide whether a phone was attached:

```powershell
  Say "== 5. Did anything change? =="
  if ($script:Device) {
      Say "  This batch was authored read-only. Confirm the phone's settings are unchanged:"
      Say "    adb shell settings list global | Select-String -Pattern 'emergency|cellbroadcast'"
      Say "    (compare against the settings task from this same batch)"
  } else {
      Say "  No phone attached, so there is nothing phone-side to compare."
      Say "  This batch was authored read-only: it reads state and writes nothing to any device."
  }
```

`$script:Device` was **never assigned anywhere in the script.** Step 3 printed the device serial with
`Good "  device: ..."` and discarded it. So on every run — with one phone attached, authorized, and
posting evidence successfully — the check took the `else` branch and told the operator:

> No phone attached, so there is nothing phone-side to compare.

That is false. A phone was attached, and the batch had just read 76 commands off it. The operator was
told the one check that would have caught a state change had nothing to check.

## Why it matters more than a cosmetic branch

This is the fourth instance of the same pattern in this repository, after BUG-012's byte-count
centrality and the false permission denial:

* BUG-009/010 in `docs/bugs/` — every signal said the work succeeded and it had not.
* The ten empty evidence files accepted because the script printed "posted" for each.
* `is_root` returning a safety-flavoured answer from an unapproved state change (BUG-024).

Here the failure mode is subtler than a wrong answer: the verification step **always ran and always
passed**, by testing a value that was always empty. A verification that cannot fail is worse than no
verification, because it is reported as a completed check. The operator sees a printed line saying
the phone was unchanged and reasonably reads it as confirmation.

It is also exactly the class of defect the existing `tools/check_paste_ps1.py` was written to catch
for other patterns — "a pipeline result indexed without `@()`" is the same idea — and this instance
was outside what it looked for.

## The fix

Step 3 now records what it found, and the comment says why the assignment is load-bearing:

```powershell
  } else {
      Good "  device: $($devices[0].Split("`t")[0])"
      # Recorded for step 5. Without this assignment step 5 tested an empty variable and always
      # reported "no phone attached", so the did-anything-change check never actually ran.
      $script:Device = $devices[0].Split("`t")[0]
  }
```

Step 5 now branches on a real value and the comparison actually happens when a phone is attached.

## Prevention

`tools/check_paste_ps1.py` gains a check: any `$script:X` that is **read but never assigned** is a
defect, because the block uses script scope only to carry state between steps, so a read of an
unwritten variable always means a step silently takes the wrong branch.

Two implementation notes, both because the first attempt was wrong:

* The obvious regex `\$script:(\w+)(?!\s*=[^=])` **backtracks** and truncates the name — `AdbPath`
  matched as `AdbPat`, which would have produced a false positive on a variable that *is* assigned.
  The check now takes the full `\w+` and inspects the remainder of the line.
* A line that *is* the assignment is skipped, so `$script:Device = ...` is not reported as a read.

The check was confirmed to **bite**: deleting the new assignment line produced
`FAIL ... $script:Device is read but never assigned`, and restoring it produced a clean pass.

## Label

`CONFIRMED`. The variable is absent from the script's assignments — a mechanical fact — and the
degenerate branch is visible in the source. The fix is verified by the paste checker, not by
executing the paste; this environment has no PowerShell.
