# BUG-011 — Polished UI referenced an undefined accent token

**Status:** FIXED  
**Severity:** HIGH  
**Affected component:** app/ui.py  
**Environment:** GitHub Actions Ubuntu UI test job

## Reproduction
Run the real widget-tree test under Xvfb:

```text
xvfb-run -a /usr/bin/python3 tools/test_ui.py
```

## Raw output

```text
NameError: name 'ACCENT_DIM' is not defined
```

## Root cause
The visual refresh introduced the new ACCENT_DIM palette value in app/widgets.py, but app/ui.py did not import it.

## Fix
Added the missing import.

## Regression test
The full tools/test_ui.py job constructs the sidebar and passes the widget-tree checks.

## Commit
6d175b7
