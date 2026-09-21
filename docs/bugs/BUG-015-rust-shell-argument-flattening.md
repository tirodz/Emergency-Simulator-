# BUG-015: the Rust controller passed the alert body as a separate `adb shell` argument

| Field | Value |
| --- | --- |
| Status | FIXED |
| Severity | HIGH |
| Affected component | `src-tauri/src/lib.rs`, `send_test_alert` |
| Environment | Any; Windows Tauri controller over adb to a controlled target |
| Discovered | Repository audit, after the Python desktop path was retired in favour of Tauri 2 + Rust |
| Commit | (this branch) |

## Summary

The Rust controller invoked the injector by handing `adb` a vector of separate arguments:

```rust
command_output(&app, &[
    "-s", &serial, "shell",
    "CLASSPATH=/data/local/tmp/alertinject.jar",
    "app_process", "/system/bin", INJECTOR_CLASS,
    &category, &package, &normalized_body,
])
```

`adb shell` does **not** preserve an argument vector. It concatenates the arguments it was given into
a single string that the *device's* shell re-parses. The controller passed no quoting of its own, so
it was relying entirely on whichever `adb` version happened to be bundled to escape each argument
before joining. That is the same defect class as BUG-001 — it was fixed in the retired Python
controller with `shlex.quote` and then not carried across into the Rust rewrite, so the Rust path was
unguarded again.

## Reproduction

Send a body containing a space or a shell metacharacter, for example:

```
TEST ALERT - SIMULATION
TEST; id
TEST $(id)
TEST it's a test
```

## Expected

The injector receives the body as exactly one argument and the device's alert shows it verbatim.

## Actual

Depends on the transport: the body can arrive as several shell words, so only the first is bound to
the injector's `argv[2]`; or metacharacters can be interpreted by the device shell. Either way
`app_process` exits 0 and, if the truncated body still begins with `TEST`, a genuine alert is
displayed — a false success with the wrong text, exactly as BUG-001 recorded.

## Root cause

The bug is a property of `adb shell`, not of any one caller. A remote command reaches Android as
**one string** that `/system/bin/sh` parses, so every argument crossing that boundary has to be
quoted by the side that knows the argument boundaries. The controller knew them and did not apply
the quoting; adb may or may not have.

## Fix

The controller now builds the whole remote command itself and passes it as a **single** `adb shell`
argument, with every injector argument POSIX-quoted by `sh_quote` in Rust:

```rust
fn injector_command_script(package: &str, body: &str) -> String {
    format!(
        "CLASSPATH={class} app_process /system/bin {main} {category} {package} {body}",
        class = INJECTOR_REMOTE,
        main = sh_quote(INJECTOR_CLASS),
        category = sh_quote(&SERVICE_CATEGORY.to_string()),
        package = sh_quote(package),
        body = sh_quote(body),
    )
}
```

`sh_quote` wraps the value in single quotes and encodes an embedded single quote as `'\''`. Inside
single quotes every other shell metacharacter is a literal, so once the embedded quotes are broken
out there is nothing left for the device shell to act on. This removes the dependency on adb's
escaping behaviour entirely.

The same treatment was applied to the preference-file push, which previously interpolated the remote
path into an `sh -c` string by hand.

## Regression test

`src-tauri/src/lib.rs` gained a `#[cfg(test)]` module with seven tests, run in CI:

* the multi-word body survives as one quoted word;
* every injector argument is quoted;
* shell metacharacters in a body are quoted and cannot be interpreted;
* an embedded single quote is escaped and round-trips;
* every printable ASCII character round-trips through the quoting;
* quotes, backslashes, newlines, `$`, non-ASCII and emoji round-trip.

The Rust tests were additionally cross-checked against a real `/bin/sh`: the exact string
`injector_command_script` produces is executed with a stub `app_process` that reports its `argv`, and
21 hostile bodies (including `;`, `|`, `$(id)`, backticks, `&`, redirects, quotes, globs, newlines
and emoji) all reach the injector at `argv[3]` byte-identical. That is the check that proves the
quoting is correct rather than merely self-consistent.

## Why this record matters

BUG-001 was recorded, fixed, regression-tested — and then lost in a rewrite. That is the reason this
record exists separately: a fix in a component that is later replaced is not a property of the
system, and the quoting rule has to be re-established wherever a new caller crosses the same
boundary. A future agent adding a transport (Wi-Fi, a new shell wrapper) must apply the same rule.
