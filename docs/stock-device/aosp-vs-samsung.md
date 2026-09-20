# Stock-device investigation: AOSP vs Samsung

This file records the current Mission 3 state without upgrading inference into device-specific fact.

## Current evidence

- **CONFIRMED — AOSP:** Android's CellBroadcast documentation identifies the default receiver package as `com.android.cellbroadcastreceiver`. The receiver is the system app responsible for emergency and nonemergency alerts.
- **CONFIRMED — Samsung official UX:** Samsung documents a user-facing **Wireless emergency alerts → Test alerts** setting on Galaxy devices. Samsung describes these as carrier/system safety-alert tests; the documentation does not expose a local third-party API that makes a phone originate a Cell Broadcast test message.
- **LIKELY — Samsung package:** Public device-management documentation says Samsung devices may use an OEM variant such as `com.samsung.android.cellbroadcastreceiver`. This is not A35-specific evidence.
- **UNKNOWN — Galaxy A35 exact package:** The A35-RO-001 read-only evidence batch has not yet been posted into this repository, so this project's exact A35 package/manifest/signature cannot yet be stated as confirmed.
- **UNKNOWN — Galaxy A35 exported injection interface:** No A35-specific evidence has yet been collected proving that Samsung exposes a legitimate, non-privileged component that accepts a test `SmsCbMessage`.
- **CONFIRMED — project safety boundary:** The AOSP protected emergency broadcast is not an ordinary application API. The project therefore must not replace the missing interface with a bypass or exploit.

## What the code now handles

The desktop controller discovers the active CellBroadcast package on the target instead of assuming a Google package. The Android injector receives that detected package from the controller and rejects package names that do not identify a CellBroadcast component.

This fixes an OEM-compatibility bug for controlled targets without changing the stock-device security boundary.

## Next unresolved step

Run the existing **A35-RO-001** read-only batch from the operator's Windows machine and record the raw package/manifest evidence here. Only after that evidence exists should the project decide whether Samsung exposes any legitimate, non-privileged test entry point.

Sources: Android Open Source Project CellBroadcast documentation; Samsung Galaxy Wireless Emergency Alerts support documentation.
