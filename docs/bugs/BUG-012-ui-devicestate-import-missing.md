# BUG-012 — Polished device renderer referenced an undefined DeviceState

**Status:** FIXED  
**Severity:** HIGH  
**Affected component:** app/ui.py  
**Environment:** GitHub Actions Ubuntu UI test job

## Reproduction
Run:

```text
xvfb-run -a /usr/bin/python3 tools/test_ui.py
```

## Raw output

```text
NameError: name 'DeviceState' is not defined
```

The exception occurred while rendering a non-root device row.

## Root cause
The polished renderer compares dev.state with the DeviceState.NO_ROOT enum, but the enum import was omitted during the rewrite.

## Fix
Added DeviceState to the UI model imports.

## Regression test
tools/test_ui.py renders both a supported controlled device and a non-root device.

## Commit
474fa5e
