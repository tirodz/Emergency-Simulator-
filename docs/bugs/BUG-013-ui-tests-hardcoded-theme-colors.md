# BUG-013 — UI regression tests kept stale hardcoded palette values

**Status:** FIXED  
**Severity:** MEDIUM  
**Affected component:** tools/test_ui.py  
**Environment:** GitHub Actions Ubuntu UI test job

## Reproduction
After the palette was moved to the shared widget theme, run the UI tests. Functional behavior passed, but colour assertions still compared against the old literal hex values.

## Root cause
The tests encoded presentation constants instead of consuming the shared theme values.

## Fix
The UI tests now import ERR and OK from app.widgets and assert against those shared constants.

## Regression test
The complete UI suite passes on the release build.

## Commit
474fa5e
