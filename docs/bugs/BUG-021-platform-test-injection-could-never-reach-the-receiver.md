# BUG-021: the platform test-injection command could never have reached the receiver

| Field | Value |
| --- | --- |
| Status | FIXED |
| Severity | HIGH |
| Affected component | `src-tauri/src/lib.rs`, `send_platform_test_alert` |
| Environment | Any Android target, every API level |
| Discovered | Overnight lead-engineer audit, verifying the injection command against AOSP source |
| Commit | (this branch) |

## Summary

The command that was supposed to exercise the AOSP test entry point constructed a broadcast that
`am` would reject before it ever reached the telephony process. Four defects, any one of which is
fatal to the attempt:

1. **`-n` was given a bare package name.** `cellbroadcast_candidates()` returns package names such
   as `com.samsung.android.cellbroadcastreceiver`. The `-n` flag takes `package/class`, or
   `package/.RelativeClass`. A package name with no slash is not a component and `am` fails with
   "Bad component name".

2. **The receiver it named is not addressable this way.** The AOSP test receiver —
   `GsmInboundSmsHandler.GsmCbTestBroadcastReceiver` — is registered *dynamically* inside the
   telephony process when `ro.debuggable=1`. It is not a manifest component, so it has no component
   name in any package to pass to `-n`, and naming the CellBroadcast package points at an unrelated
   receiver. AOSP's own documented invocation broadcasts on the action alone:

   ```
   adb shell am broadcast -a com.android.internal.telephony.gsm.TEST_TRIGGER_CELL_BROADCAST \
     --es pdu_string <hex> --ei phone_id 0
   ```

3. **A stray `--es format 3gpp` extra.** `pdu_string` is already the encoded PDU. `format` is not a
   key the handler reads, so it added an extra whose only effect was clutter in the record.

4. **`--receiver-foreground`** applies to a registered component; for an action-only broadcast to a
   dynamically registered receiver it changes nothing and misleadingly implies a component target
   was supplied.

Because defect 1 fails at argument parsing, the send reported `accepted_by_am=false` immediately —
and, since exit status is not evidence of delivery, the plausible failure it produced was
indistinguishable from the receiver simply not being present. The feature could not have worked on
any device, including the `userdebug` targets where the entry point genuinely exists. This is the
same false-success family as BUG-001 and BUG-015, one layer further in: instead of a truncation that
looked like success, a structurally impossible command that looked like a device limitation.

## Reproduction

On a `userdebug` target with a CellBroadcast package installed, run the platform test-injection
command. `am` exits non-zero with a bad-component parse error and no broadcast reaches
`GsmInboundSmsHandler`. On a `user` build the same command fails identically to how it fails when the
entry point is legitimately absent, which is what made it look like a device property.

## Root cause

ROOT CAUSE: the action name and its argument contract were reconstructed from the intent *action*
constant alone, without checking how AOSP actually invokes it. Taking a package-name list — which is
the right abstraction for the `am start`-style OEM receiver path (BUG-016) — and feeding its first
element to `-n` conflated two different addressing schemes: an explicit manifest component, and an
action broadcast to a dynamically registered receiver.

## Fix

* `-n`, `--receiver-foreground`, and `--es format 3gpp` removed; the command matches the AOSP
  example.
* `--ei phone_id 0` added, selecting the default subscription.
* Acceptance now checks the `am` output for `Broadcast failed` as well as the exit code, because `am`
  can report a missing receiver on stdout while still exiting 0 — the exit code alone would be the
  BUG-007 failure again.
* The comment records *why* there is no `-n`, so the next maintainer does not "helpfully" add one
  back.

## Regression test

Covered by the argument-construction test in this commit; the command is asserted to contain the
action, `--es pdu_string`, and `--ei phone_id 0`, and to contain no `-n`. The remaining uncertainty —
whether a given OEM build registers the test receiver at all — is deliberately left as `BLOCKED` or
`UNKNOWN` rather than being papered over with a second guess.

## Verification status

INFERRED from the AOSP source and its documented example. Not CONFIRMED: no `userdebug` target has
been driven here, and this environment has no adb and no radio.
