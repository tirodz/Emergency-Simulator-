# Stock Galaxy A35 security boundary

## Status

**BLOCKED ON DEVICE EVIDENCE**

The project has not yet received the raw A35-RO-001 evidence. This means the exact Samsung package, declared permissions, exported components, APEX layout, and shell identity on the physical A35 remain unverified in this repository.

## Established facts

**CONFIRMED — AOSP:** `android.provider.action.SMS_EMERGENCY_CB_RECEIVED` is the protected emergency CellBroadcast entry action in the AOSP platform. The CellBroadcast receiver is a privileged system component.

**CONFIRMED — project test target:** On the project's Android 15 userdebug test environment, the controlled injector reached the genuine CellBroadcast receiver and the production alert UI. The controller treats downstream receiver/service/dialog evidence as the proof of delivery.

**CONFIRMED — stock boundary in the tested AOSP model:** An ordinary shell/application identity is not equivalent to the accepted system identity for the protected broadcast.

**UNKNOWN — exact Samsung implementation:** The A35 must be inspected before claiming that Samsung uses the same package, component names, permission protection levels, or vendor-specific test hooks.

## Explicitly out of scope

The project will not attempt to defeat a signature permission, bypass SELinux, obtain root, unlock the bootloader, replace Samsung components, or use a newly discovered vulnerability to originate an emergency alert on stock firmware.

A security weakness may be **characterized and documented** as a finding, but not turned into an operational bypass.

## Mission 3 decision rule

1. Collect the existing read-only A35 evidence.
2. Inspect the receiver manifest and protection levels.
3. Look for a vendor-supported, non-privileged test interface.
4. If one exists, test that interface with explicit approval.
5. If none exists, record the legitimate boundary and keep the real alert path available on controlled development/root targets.

Sources: AOSP CellBroadcast architecture/security documentation; repository Mission 2 experiments.
