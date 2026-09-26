# BUG-024 — device discovery restarted adbd as root without approval

**Status:** FIXED
**Class:** safety-boundary violation, not a functional defect
**Found:** 2026-09-22, overnight investigation session
**File:** `src-tauri/src/lib.rs` (`is_root`, called from `list_devices`)

## What the code did

Device discovery called this once per device, on every refresh:

```rust
fn is_root(app: &tauri::AppHandle, serial: &str, build_type: &str) -> bool {
    // Never ask production/user Samsung builds to restart adbd as root.
    if build_type.eq_ignore_ascii_case("user") {
        return false;
    }

    let _ = adb_call(app, &["-s", serial, "root"]);
    thread::sleep(Duration::from_millis(700));
    ...
}
```

`adb root` is not a query. It instructs the device's `adbd` daemon to restart **running as root**.
The comment's guard is a `user`-build check, and that check is what makes the defect easy to miss:
it reads as a safety measure, so a reviewer sees a protected call rather than an unapproved state
change. On a `userdebug` or `eng` build — which is exactly what a controlled development target is —
the call is not skipped.

## Why it violates the project's own constraint

AGENTS.md states:

> Read-only first; anything that could change phone state needs the operator's explicit approval for
> that specific command.

Restarting adbd as root on the operator's phone is a change to phone state. It was not approved for
any specific command, it was not announced in the UI, and it happened as a side effect of pressing
Refresh. The `note` string in the paste tells the operator the batch "changes nothing on the phone",
which was true of the batch and false of the controller driving it.

## Why it was also pointless

The call appears to be there to enable the controlled test path. It does not:

* Whether the AOSP test receiver exists is decided by `ro.debuggable` **at class initialisation**.
  Root cannot change a build property that was read at process start. See
  `docs/stock-device/aosp-test-entrypoint.md`.
* The receiver is registered `Context.RECEIVER_EXPORTED` with no permission, so `uid=2000` (the
  ordinary adb shell) may already target it. Root adds nothing to that delivery.
* The one path that genuinely needs elevated storage — the legacy injector at
  `/data/local/tmp` — is an install action with its own command, not something discovery should be
  preparing.

So the call changed the operator's phone to enable something it could not enable.

## The fix

`is_root` is replaced by `adbd_uid`, which reads the current uid and changes nothing:

```rust
fn adbd_uid(app: &tauri::AppHandle, serial: &str) -> Option<u32> {
    shell(app, serial, &["id", "-u"])
        .ok()
        .and_then(|text| text.trim().parse::<u32>().ok())
}
```

The device's `root` field now means "adbd is *already* running as root", which is a description, not
an action. A non-zero uid is the normal answer and is not treated as a failure.

The controlled path is now gated on the capability that actually decides it:

```rust
} else if test_entrypoint.available == PlatformState::Granted {
```

This is a correctness improvement as well as a safety one. Previously a **rooted `user` build** was
reported `READY`/`Supported`, because `root && cellbroadcast_package.is_some()` was the condition —
but such a build has no test receiver and the broadcast cannot be delivered. A `userdebug` build is
now correctly `READY` regardless of whether anyone ran `adb root`, because the exported receiver is
reachable from the ordinary shell.

## Prevention

`tools/check_read_only.py` fails if any state-changing ADB invoker reappears in the Rust sources:
`root`, `unroot`, `remount`, `reboot`, `uninstall`, `install-multi-package`, `pm clear`, `settings
put`, `svc` radio toggles, and wipe/format verbs.

It is not a proof — it cannot see a command assembled at runtime — and it is deliberately narrow: it
does not ban `adb shell`, which is how the controller reads the device. It catches the direct
invocations, which is how this defect appeared.

The check was confirmed to **bite**: appending the removed `adb root` line back produced
`exit 1` naming the line, and removing it produced `exit 0`. A guard that cannot fail is the exact
false-success pattern this repository keeps recording.

## Label

`CONFIRMED` as a boundary violation — the call is visible in the source and its effect on a
`userdebug` device is documented Android behaviour. The fix is verified by unit tests and the
static guard; it has not been exercised against a phone, because this environment has no adb.
