# BUG-008: `default_log_dir()` referenced a removed constant

| Field | Value |
| --- | --- |
| Status | FIXED |
| Severity | MEDIUM |
| Affected component | `app/controller.py`, `default_log_dir` |
| Environment | Repository checkout, any platform |
| Discovered | Immediately, by running the CLI after the runtime refactor |
| Commit | `7c4e857` |

## Summary

Moving path resolution into `app/runtime.py` removed the module-level `REPO_ROOT` constant, but
`default_log_dir()` still referenced it. Every CLI invocation that was not `--adb-info` raised
`NameError` at startup.

## Reproduction

```
python3 tools/test_alert.py --list
```

## Expected

The device list is printed.

## Actual

```
NameError: name 'REPO_ROOT' is not defined
```

## Raw output

```
  File "/workspace/project/Emergency-Simulator-/app/controller.py", line 792, in default_log_dir
    return REPO_ROOT / "logs"
NameError: name 'REPO_ROOT' is not defined
```

## Root cause

A refactor that centralises path logic left one stale reference to a constant it deleted. Python
resolves module-level names at call time, so the error surfaced only when that function was reached,
not at import.

## Fix

`default_log_dir()` now asks the runtime layout for the root, and the frozen and checkout branches
are both expressed in terms of it. It returns `search_roots[-1] / "logs"` in a checkout, which is the
repository root by construction rather than by a duplicated constant.

## Regression test

`tools/test_alert.py --list` and `tools/test_controller.py` both exercise the logging setup on every
run, so any recurrence fails immediately. The compile check
`python3 -m py_compile app/*.py tools/*.py` runs in CI.

## Lesson

Small. Recorded because it is the one bug in this journal that the existing test suite caught on its
own, which is worth noting: the tests did their job. It also shows why the compile check belongs in
CI even for an interpreted language.