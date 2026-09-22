# BUG-023: fixed-width device images clipped inside a shrinking container

| Field | Value |
| --- | --- |
| Status | FIXED |
| Severity | LOW |
| Affected component | `src/index.html`, `.studio-phone img` and `.device-preview-art img` |
| Environment | Windows desktop controller, window width at or below ~1180px |
| Discovered | By the corrected UI harness (BUG-022) on its first real run |
| Commit | (this branch) |

## Summary

The hardware render images were sized in fixed pixels inside `overflow: hidden` boxes that shrink
with the window:

```css
.studio-phone { height: 300px; overflow: hidden; }
.studio-phone img { width: 235px; height: 275px; object-fit: contain; }
```

At a 1000px viewport the studio column narrows to about 208px while the image stays 235px. Because
the box has `overflow: hidden`, the excess is clipped rather than scrolled, so the render loses its
right edge and the layout reports no error at all — the document never overflows. `.device-preview-art
img` had the same shape at 128px.

This is the mild version of the same blind spot as BUG-022: clipping inside a box is invisible to a
document-level measurement, and invisible to the operator as anything other than a slightly cropped
picture.

## Reproduction

Open the devices studio with the window at 1000px wide. The device render is cut off on the right.
The corrected harness reports `div.studio-phone scroll=235 client=208`.

## Root cause

ROOT CAUSE: a fixed pixel width on a replaced element inside an `overflow: hidden` box with no
`max-width`. Nothing constrains the image to its container's width.

## Fix

`max-width: 100%` (and `max-height: 100%` for the studio render) added to both images, so the render
scales down with its container instead of being clipped. `object-fit: contain` already preserves the
aspect ratio.

## Regression test

`tools/check_ui.mjs` asserts no element under the studio profile exceeds its own box at 1000, 1180,
1360, and 1600px.
