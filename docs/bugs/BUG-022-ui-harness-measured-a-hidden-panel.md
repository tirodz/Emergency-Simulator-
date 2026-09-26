# BUG-022: the UI regression harness measured a hidden panel

| Field | Value |
| --- | --- |
| Status | FIXED |
| Severity | HIGH |
| Affected component | `tools/check_ui.mjs` |
| Environment | The harness itself |
| Discovered | Overnight audit, while trying to prove the harness could fail |
| Commit | (this branch) |

## Summary

The harness was written to prove the capability UI does not overflow. It navigated the page, clicked
Refresh, and measured `document.documentElement.scrollWidth` on every assertion. Every overflow check
passed on the first run and kept passing.

None of them was measuring anything. The application loads into `view-overview`, and
`body.view-overview #devicesCard { display: none !important }` hides the entire devices card —
including `#deviceDetail`, the panel the assertions were aimed at. Every element under test had
`clientWidth: 0` and `getBoundingClientRect().width: 0`. A zero-width element cannot overflow.

The harness reported `ok [1000px stock-a35] no horizontal overflow` for a layout that was never laid
out. This is the same failure pattern as BUG-001 and BUG-015 — a green signal that means nothing —
committed by the tool built to catch it. It is recorded separately because the tool is the thing that
is supposed to be trustworthy.

## Reproduction

Run `node tools/check_ui.mjs` against the pre-fix harness and add `console.log` of
`getComputedStyle(document.getElementById("devicesCard")).display`. It prints `none` for every
assertion that reports `no horizontal overflow`.

## Root cause

Two compounding mistakes:

1. **No visibility precondition.** The harness assumed the target panel was rendered. It never
   asserted `display !== none` before measuring. A hidden element satisfies every width inequality.
2. **Only a document-level metric.** `scrollWidth` on the document root cannot see text clipped
   *inside* its own box. The original UI defect was `white-space: nowrap` plus
   `text-overflow: ellipsis` on `.kv span`: the text was truncated within the element, the element
   stayed inside the viewport, and the document never overflowed. Even had the panel been visible,
   the measurement would have stayed green on the exact bug it was written for.

## Fix

* The harness now clicks the `devices` navigation entry and asserts the card became visible before
  measuring. A hidden panel is a hard failure, not a pass.
* A per-element clipping check was added: any element under `#deviceDetail`, `#diagPanel`, or the
  studio profile whose `scrollWidth` exceeds its `clientWidth` is a failure, except declared
  scrollable elements (`PRE`, `TEXTAREA`, i.e. the raw logcat dump).
* An overflow-detector self-test injects an in-flow 5000px element and requires the detector to
  notice. If the detector is dead, the run fails.

## What the fixed harness immediately found

The corrected checks failed on first run, which is the point:

* `div.studio-phone scroll=235 client=208` at 1000px — BUG-023.
* `span. scroll=442 client=282 nowrap=nowrap` — the original `.kv span` clipping, reproduced.

Both are now fixed and the harness is green because the layout is correct, not because the
assertions are inert.

## Note on the self-test

The first version of the self-test injected a `position: fixed` element. Fixed elements are removed
from flow and do not contribute to `scrollWidth`, so the probe did not overflow the document and the
self-test failed — correctly reporting that the detector had not fired. Using an in-flow element made
it a valid control. The self-test failing first, rather than passing, is how the dead detector was
caught; it is worth keeping for that reason.

## Regression test

`tools/check_ui.mjs`, run via `npm run test:ui`. It fails on a hidden panel, on document overflow, on
in-box clipping, and on a dead overflow detector.
