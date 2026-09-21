# BUG-016: the controller assumed exactly one CellBroadcast package and never retried another

| Field | Value |
| --- | --- |
| Status | FIXED |
| Severity | MEDIUM |
| Affected component | `src-tauri/src/lib.rs`, `find_cellbroadcast`, `send_test_alert` |
| Environment | Any OEM distribution that ships more than one CellBroadcast package |
| Discovered | Repository audit, while addressing OEM receiver targeting |
| Commit | (this branch) |

## Summary

Package discovery sorted the packages whose names contain `cellbroadcast` by how specifically they
named a receiver, then took the **first one only**:

```rust
packages.into_iter().next()
```

The controller sent to that one package and no other. On a distribution that carries both a
Google/Mainline module and a vendor build — the Galaxy A35 is a plausible example — whichever sorted
first was tried, and a rejection of that package ended the attempt. The other candidate was never
addressed, so an alert that the device would have accepted through its second receiver was reported
as a failure, or (worse) as no evidence at all.

## Reproduction

On a controlled target with two CellBroadcast packages installed, send an alert. If the package the
sort selected does not accept the broadcast, the controller reports `FAILED` and nothing else is
tried.

## Expected

Every plausible receiver the device exposes should be considered, in a deterministic order, and a
rejection of one should not end the attempt.

## Actual

One package tried; a rejection was terminal.

## Root cause

Discovery was written as "find the receiver", a single-valued question, when what the device actually
presents is a **set** of candidates and the only authority on which one is live is the device itself.
The controller had no way to represent more than one.

## Fix

* Discovery now returns an ordered candidate list (`cellbroadcast_candidates`), ranked by name
  specificity and then alphabetically so the order is stable rather than dependent on `pm list`
  ordering.
* The `Device` record carries the full list, so the interface can show every candidate.
* `send_test_alert` tries the candidates in order, and advances to the next one **only** on an
  explicit `BROADCAST_REJECTED` — never on a timeout or on a plain absence of evidence.

The last restriction is deliberate and is the part that matters. Retrying on a timeout would risk
sending a second alert to a device that may already be displaying the first (BUG-006). A rejection,
by contrast, means the platform refused the broadcast before any receiver saw it, so nothing is on
screen and the next candidate is safe to try. The evidence collector was also adjusted to read the
rejection signal before its poll sleep, so a fallback costs no extra delay.

## Regression test

Covered by the CI source contract (`grep -q "cellbroadcast_candidates"`), and by the ordering being a
pure function of the package list. The retry policy itself is exercised on a device: the fallback
branch is only reachable when the platform rejects a protected broadcast, which requires a target
with two receivers.

## Lesson

Where a device is the authority on which of several components is live, the controller should carry
the whole set and let the device choose, rather than guessing one and treating its failure as final.
The candidate list also makes the OEM question visible instead of hidden inside a sort.
