# EXP-16 — Mission 2B: is the release self-contained, and does the send gate hold?

**Status:** COMPLETE. Both questions answered with evidence.

## Objective

1. Determine whether a released executable can drive a device with no Python, no Android SDK, no JDK
   and no configuration on the host.
2. Verify that the interface's send gate prevents a second alert from stacking on a device that is
   already showing one.
3. Verify that a result can never overwrite the permanent safety statement.

## Hypothesis

* The bundled injector and bundled Platform Tools are both discoverable through the runtime layout, so
  the released executable is self-contained. **Confirmed.**
* The gate blocks a second send before the device is touched. **Confirmed.**

---

## Experiment 16A — the release resolves its own resources

### Setup

PyInstaller onefile build of `packaging/Emergency-Simulator.spec` with the injector and staged
Platform Tools bundled. Run from `/tmp/frozen_test`, an empty directory that is not the repository,
with `ADB_PATH` explicitly removed from the environment.

### Command

```bash
cd /tmp/frozen_test
env -u ADB_PATH /workspace/project/Emergency-Simulator-/dist/Emergency-Simulator --selftest
```

### Actual result

```
Emergency-Simulator self-test
  version     : 1.0.0
  frozen      : True
  bundle root : /tmp/_MEI00004316fqxBqu
  search root 0: /tmp/_MEI00004316fqxBqu
  search root 1: /workspace/project/Emergency-Simulator-/dist
  injector    : FOUND  /tmp/_MEI00004316fqxBqu/android/alertinject/out/alertinject.jar  (3385 bytes)
  bundled adb : /tmp/_MEI00004316fqxBqu/platform-tools/adb
  adb         : BUNDLED  /tmp/_MEI00004316fqxBqu/platform-tools/adb
  adb version : Android Debug Bridge version 1.0.41

RESULT: OK
```

The same executable was then launched normally and discovered the live emulator, logging to
`~/.emergency-simulator`:

```
INFO  Querying adb for attached devices
INFO  Found 1 device(s); 1 ready for test alerts
```

### Conclusion

The release is genuinely self-contained: with no environment help it finds its own adb, proves the adb
runs, and reaches the device. This closes the failure mode of BUG-004, where a packaged build derived
its paths from an assumption about the packaging mode rather than from what the bundle contained.

---

## Experiment 16B — the same, on real Windows

### Setup

GitHub Actions `windows-latest`, run `35449434829`. The workflow stages Platform Tools from Google's
official repository, builds with PyInstaller, then runs the artifact with `--selftest` from a
directory that is not the repository (`Start-Process -Wait`, since a GUI-subsystem binary does not
block a direct invocation — see BUG-010).

### Actual result

```
Emergency-Simulator self-test
  version     : 1.0.0
  frozen      : True
  bundle root : C:\Users\RUNNER~1\AppData\Local\Temp\_MEI000023142
  search root 0: C:\Users\RUNNER~1\AppData\Local\Temp\_MEI000023142
  search root 1: D:\a\Emergency-Simulator-\Emergency-Simulator-\dist
  injector    : FOUND  C:\...\_MEI000023142\android\alertinject\out\alertinject.jar  (3385 bytes)
  bundled adb : C:\...\_MEI000023142\platform-tools\adb.exe
  adb         : BUNDLED  C:\...\_MEI000023142\platform-tools\adb.exe
  adb version : Android Debug Bridge version 1.0.41
RESULT: OK
```

Artifact: `Emergency-Simulator-windows` / `Emergency-Simulator.exe`. Job conclusion: **success**.

### Conclusion

The self-contained build is confirmed on the actual target platform, by the platform's own CI,
against the artifact that is uploaded. `adb.exe` resolving as `BUNDLED` also confirms that the
companion DLLs (`AdbWinApi.dll`, `AdbWinUsbApi.dll`) were staged correctly — without them adb fails at
load time and this line would read `MISSING`.

---

## Experiment 16C — the send gate against a live device

### Setup

The real interface, built under Xvfb, driving the real controller against `emulator-5554`
(Android 15 / API 35, userdebug). The gate was acknowledged first to clear any leftover state, then
the send path was exercised exactly as the SEND button does, with the confirmation dialog accepted.

### Actual result

```
device: emulator-5554 SUPPORTED

RESULT PILL : ALERT DISPLAYED
GATE        : DELIVERED
SEND ENABLED: False
ACK ENABLED : True
SAFETY      : TEST ONLY      CONTROLLED DEVICE      NO CELLULAR TRANSMISSION
OUTCOME     : GENUINE ALERT DISPLAYED ON emulator-5554 — DISMISS IT ON THE DEVICE

   SUCCESS: the genuine Android emergency alert was displayed.
     CellBroadcastReceiver.onReceive -- message accepted by the receiver
     CellBroadcastAlertService.onStartCommand -- alert service started
     CellBroadcastAlertAudio -- genuine alert audio engaged
     CellBroadcastAlertDialog -- genuine full-screen alert displayed
   Dismiss it with the alert's own on-device control.
   Remote dismissal is not supported by Android.
```

### Conclusion

Three things are confirmed together:

* **The chain is still genuine.** Every component is the production one: the stock receiver accepted
  the message, the alert service started, real alert audio engaged, and the real full-screen dialog
  was displayed. The success verdict is derived from those logcat lines, not from the injector's exit
  code.
* **The gate holds.** After delivery the device is `DELIVERED`, SEND is disabled and Acknowledge is
  offered. A second send is refused with `DUPLICATE_SEND_BLOCKED` before the device is touched
  (asserted in `tools/test_controller.py`; observed earlier against the live device).
* **The safety statement survived a result.** It reads exactly as it does at rest, while the outcome
  appears on its own strip. This is the property BUG-009 was raised for, and it is asserted by test.

---

## Threats to validity

* The Windows build was verified to resolve its resources and to start; it was **not** driven against
  a device on Windows, because CI has no device. The device-driving path is identical Python and was
  exercised on Linux, but the Windows adb transport itself is untested end to end.
* Vibration, lock screen and DND override remain unverified — that is `EXP-ALERT-002`, still open.
* The gate's `UNCERTAIN` state was exercised synthetically (a timeout) rather than by an actual
  timeout against a device.

---

## Safety

No cellular transmission occurred. No modem, radio or RF path was involved. Every alert was labelled
`TEST`, no hazard category was reachable, and nothing was sent without explicit operator confirmation.
The failed first CI run and the corrected second run both only built and inspected an executable; no
device was involved.