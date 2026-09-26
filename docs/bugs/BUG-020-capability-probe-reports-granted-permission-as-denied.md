# BUG-020: a granted notification permission was reported as denied, and an unreadable one as a denial

| Field | Value |
| --- | --- |
| Status | FIXED |
| Severity | HIGH |
| Affected component | `src-tauri/src/lib.rs`, `simulator_capabilities` and its caller in `send_test_alert` |
| Environment | Any device where the local simulator is installed, Android 13+ (API 33+) |
| Discovered | Overnight lead-engineer audit, from the reported "capability probe is lying" symptom |
| Commit | `b366736` |

## Summary

The capability probe misreported `POST_NOTIFICATIONS` in two independent ways, and the second one
turned the first from a display bug into a blocked send.

The grant was read from a single line:

```rust
dump.lines()
    .find(|line| line.contains("android.permission.POST_NOTIFICATIONS"))
    .map(|line| line.contains("granted=true"))
```

`dumpsys package` prints the permission name several times. The first occurrence is normally in the
`requested permissions:` block, where the name appears with no grant state on the line at all. The
`find` therefore matched a line that could never contain `granted=true`, `map` returned `false`, and
the result was `Some(false)` — an explicit denial, for a permission that was in fact granted.

Two further defects in the same expression:

* **`find` vs `any`.** Because it took the *first* matching line, a later authoritative
  `granted=true` line could never win. Even on a dump whose first mention happened to carry the
  state, the expression was looking at the wrong line by construction.
* **Unknown defaulted to denial.** `Option<bool>` was then tested directly:

  ```rust
  if capabilities.post_notifications == Some(false) { /* block the send */ }
  ```

  and it was not only blocked, it was blocked with the instruction "Notification permission is not
  granted ... Grant notifications for that app on the phone, then retry." An operator who followed
  that instruction found the permission already granted, could change nothing, and had no path to a
  working alert. The tool was confidently wrong in the direction that wastes the operator's time.

The sibling probe had the mirror-image error. `USE_FULL_SCREEN_INTENT` mapped an absent or
unrecognised app-op to `true` — "treat as unrestricted", with a comment admitting it was a guess —
on the one op whose Android 14+ default is denial. The default was inverted relative to reality.

## Reproduction

Install the local simulator on Android 13+. Grant notifications. Run any send.

Expected: the probe reports the grant and the send proceeds.

Actual: `simulator_capabilities` reports `POST_NOTIFICATIONS` denied, and `send_test_alert` fails at
`CAPABILITY_CHECK` with `NOTIFICATION_PERMISSION_DENIED`, before any broadcast is attempted. The
permission is genuinely granted; nothing the operator does changes the verdict.

## Root cause

Three separate mistakes, each sufficient on its own:

1. A line-oriented `find` used where the property is spread across several lines.
2. A missing value conflated with a negative value by an `Option<bool>` representation that had no
   way to say "not determined".
3. An absent app-op read as permissive when the platform default is restrictive.

## Fix

* `SimulatorCapabilities` now carries `platform::State` (`GRANTED` / `DENIED` / `NOT_PRESENT` /
  `UNKNOWN` / `ERROR`) instead of `Option<bool>`. There is no representation left that forces a
  missing value to look like a denial. `State::default()` is `Unknown` for the same reason.
* Parsing moved to `platform.rs` and takes the *strongest* claim across the whole dump rather than
  the first matching line: candidates are collected with `filter`, not `find`.
* `POST_NOTIFICATIONS` is not probed below API 33, where it does not exist, instead of being
  reported as denied.
* Only `Denied` blocks the send. `Unknown` warns and continues.
* `USE_FULL_SCREEN_INTENT` treats an absent or unrecognised op as `Unknown`, not as unrestricted,
  and a denial is surfaced as a presentation warning (heads-up instead of full-screen).

## Regression test

`src-tauri/src/platform.rs` unit tests: a synthetic `dumpsys` transcript whose first mention of the
permission carries no state and whose second carries `granted=true` must parse as `GRANTED`; a
transcript mentioning it only in the `requested permissions:` block must parse as `UNKNOWN`, not
`DENIED`. Run with `cargo test --manifest-path src-tauri/Cargo.toml --lib` (53 tests).

## Note on the premise

The send is not blocked for a device without root, and never was. The probe is a probe. The
false-alarm path was confined to `DeviceState::SimulatorReady`, where the root requirement does not
apply at all.
