# Windows controller

The Emergency-Simulator desktop application triggers a **TEST** emergency alert through Android's
genuine Cell Broadcast pipeline on a rooted device you control. It performs no cellular transmission.

This document covers architecture, installation, the ADB requirement, the root requirement, the
supported test environment, building the injector and the app, running the executable, the safety
restrictions, CANCEL semantics, and troubleshooting.

For the underlying Android mechanism, see [aosp-test-path.md](aosp-test-path.md) and
[experiments.md](experiments.md). For the feasibility picture, see [feasibility.md](feasibility.md).

---

## 1. What the application does

```
Tkinter UI  (app/ui.py)
    |
    v
Controller  (app/controller.py)      <- validation, preparation, evidence detection
    |
    v
ADB layer   (app/adb.py)             <- argument-array subprocess calls, no shell strings
    |
    v
AlertInjector.jar  (android/alertinject/)   <- pushed to the device, run via app_process
    |
    v
SmsCbMessage  ->  android.provider.action.SMS_EMERGENCY_CB_RECEIVED
    |
    v
stock CellBroadcastReceiver -> CellBroadcastAlertService
    |
    v
REAL emergency-alert UI + alert audio/TTS + history database
```

The same controller backs both front ends, so `tools/test_alert.py` and the GUI do provably the same
thing. Verified end to end against `emulator-5554` (Android 15 / API 35, userdebug): the alert
reached `CellBroadcastReceiver`, `CBAlertService`, `CellBroadcastAlertAudio` and
`CellBroadcastAlertDialog`, and a row was written to the genuine history database.

---

## 2. Requirements

| Requirement | Why |
| --- | --- |
| Windows 10/11 (or Linux/macOS) | The primary target is Windows; the core is portable Python. |
| Python 3.10+ with tkinter | The packaged `.exe` bundles this, so end users need nothing. |
| Android Platform Tools (`adb`) | Transport to the device. See section 3. |
| A **rooted** Android device | The emergency broadcast is a protected broadcast; a non-root sender is rejected. |
| USB debugging enabled | For adb to reach the device. |

The device must be one you own and control. Do not point this tool at a device you do not administer.

---

## 3. ADB setup

The application never downloads binaries. Obtain Platform Tools from the official source:

**https://developer.android.com/tools/releases/platform-tools**

Then do one of the following.

1. Put `adb` on your `PATH`.
2. Set `ADB_PATH` to the executable:
   ```powershell
   $env:ADB_PATH = "C:\platform-tools\adb.exe"
   ```
3. Pass it explicitly on the command line: `--adb C:\platform-tools\adb.exe`

Resolution order: `--adb`, then `ADB_PATH` (file or directory), then `PATH`, then the conventional
SDK locations (`%LOCALAPPDATA%\Android\Sdk\platform-tools`, `$ANDROID_HOME`, `$ANDROID_SDK_ROOT`).

If adb cannot be found the app says so and stops; it does not guess.

---

## 4. Root requirement

Root is required, and the tool refuses to send without it. This was established experimentally: a
shell-UID attempt produced

```
Permission Denial: not allowed to send broadcast
  android.provider.action.SMS_EMERGENCY_CB_RECEIVED from uid=2000(shell)
```

Two ways to satisfy it:

* **userdebug/eng build** (an emulator, or a development device): `adb root` makes the adbd shell
  itself uid 0. This is what the verification environment uses.
* **rooted retail device**: `su` escalates per command. The controller detects this and uses it.

The controller tries `adb root`, then `su`, and reports `NO_ROOT` if neither works.

---

## 5. Supported Android test environment

| Item | Value |
| --- | --- |
| Android version | 15 / API 35 (verified) |
| Build type | `userdebug` (`ro.build.type=userdebug`, `ro.debuggable=1`) |
| Device | AVD `test35`, `google_apis/x86_64` |
| CellBroadcast package | `com.google.android.cellbroadcastreceiver` |

The AOSP package name `com.android.cellbroadcastreceiver` is also probed, and a listing fallback
looks for any `*cellbroadcastreceiver*` package so OEM renames are tolerated.

---

## 6. Building the injector

The Android side is a Java tool dexed into a jar and run through `app_process`. It is **not** rebuilt
by the Python code on Windows; build it once on a machine with the Android SDK and a JDK.

```bash
android/alertinject/build.sh          # needs ANDROID_HOME and JAVA_HOME
# produces android/alertinject/out/alertinject.jar
```

From the repository the controller builds it automatically when the jar is missing or older than its
sources. In a **packaged** build there are no sources, so the jar must be placed next to the
executable:

```
Emergency-Simulator.exe
android/alertinject/out/alertinject.jar
```

---

## 7. Building the Windows application

**Locally, on Windows:**

```powershell
powershell -ExecutionPolicy Bypass -File packaging\build_windows.ps1
```

This creates a `.venv`, installs PyInstaller, runs the safety tests, and builds
`dist\Emergency-Simulator.exe`. Use `-SkipTests` to skip the tests, `-NoVenv` to build with the
system interpreter.

**With CI:** the workflow `.github/workflows/build-windows.yml` builds the executable on
`windows-latest` and uploads it as the artifact **`Emergency-Simulator-windows`** (file:
`Emergency-Simulator.exe`). It runs on push to `main`, pull requests, and manual dispatch. Download
it from the run's Artifacts section.

Generated binaries are not committed; the CI artifact is the distribution route.

---

## 8. Running it

```powershell
$env:ADB_PATH = "C:\platform-tools\adb.exe"
.\dist\Emergency-Simulator.exe
```

Command line, for the same engine without a GUI:

```powershell
python tools\test_alert.py --list
python tools\test_alert.py --dry-run
python tools\test_alert.py --device emulator-5554
python tools\test_alert.py --device emulator-5554 --message "TEST DRILL - HOUSEHOLD DEVICE" --yes
```

Exit codes: `0` success, `1` failure, `2` usage error, `3` refused by a safety rule, `4` cancelled.

The GUI starts by scanning for devices. Select one, check the status block, then use **Dry Run** or
**SEND TEST ALERT**.

---

## 9. Safety restrictions

These are enforced in code, not by convention.

* **The channel is fixed.** The service category is a module constant,
  `SERVICE_CATEGORY = 4355` (0x1103, the ETWS test channel). There is no parameter, config key, API
  field, environment variable or CLI flag that can change it. `tools/test_controller.py` asserts both
  the value and the absence of any option that would select a real hazard class.
* **The body must be a test.** It must begin with `TEST`; anything else is refused before any device
  is touched. An empty body becomes the default rather than being sent. Lowercase `test` is not
  accepted, so the on-screen text is never ambiguous.
* **Nothing is silent.** The CLI prompts for an explicit `y`/`yes`; anything else cancels. The GUI
  uses a confirmation dialog that repeats target, build, type, locked channel and message, and
  defaults to "no".
* **Nothing transmits.** There is no modem, radio or RF code path. The message is injected locally
  into the receiver on the device; it never reaches a network.
* **Nothing can be scheduled.** There is no timer, queue or retry; each send is one explicit action.

Not exposed anywhere: presidential alerts, AMBER, CBRNE hazard classes, real government warning
categories, or arbitrary emergency categories.

---

## 10. CANCEL semantics

The STOP button means different things before and after delivery, and the application says so rather
than pretending otherwise.

| Phase | What STOP does |
| --- | --- |
| Before delivery (discovery, preparation, before injection) | Cancels the pending local operation. Nothing is delivered. |
| While waiting for evidence | Stops waiting and reports what was seen. |
| After the alert is displayed | **Nothing.** Android does not permit remote dismissal. |

After delivery the controller reports:

```
Alert already delivered.
Remote dismissal is not supported.
Dismiss using the alert's own on-device control.
```

This matches the experiment: BACK presses are swallowed by an `OnBackInvokedCallback` the alert
window registers deliberately, and `CLOSE_SYSTEM_DIALOGS` is ignored. The tool does not attempt BACK
hacks, `CLOSE_SYSTEM_DIALOGS`, protected-broadcast tricks, or any security bypass.

---

## 11. How success is decided

A clean injector exit code is **not** success. The controller clears logcat, injects, then polls for
the production components and requires meaningful downstream evidence:

| Evidence | Meaning |
| --- | --- |
| `CellBroadcastReceiver.onReceive` | The receiver accepted the message. |
| `CBAlertService.onStartCommand` | The alert service started. |
| `CellBroadcastAlertAudio` | Genuine alert audio engaged. |
| `CellBroadcastAlertDialog` | The genuine full-screen alert was displayed. |

`ALERT_DISPLAYED` requires the dialog. Failure codes include `DEVICE_OFFLINE`, `NO_ROOT`,
`TEST_MODE_DISABLED`, `INJECTOR_FAILURE`, `BROADCAST_REJECTED`, `CELLBROADCAST_FILTERED`,
`ALERT_PROCESSING_FAILED` and `TIMEOUT`.

---

## 12. Test-mode preparation

The receiver only accepts a test alert when two preferences are set: `testing_mode` and
`enable_test_alerts`. The controller establishes them, preferring the vendor mechanism:

```
adb shell am broadcast -a android.telephony.action.SECRET_CODE -d "android_secret_code://2627"
```

That action is a **toggle, not a setter**. The controller therefore reads the current state first and
only sends it when testing mode is off; sending it blindly twice would disable a working setup.
Whatever the secret code leaves unset is written into the receiver's preference file, and the
receiver is force-stopped so it re-reads its configuration.

If the preference file cannot be read, the controller **refuses** rather than guessing, because a
wrong guess on a toggle-style setting could break a working configuration. `--dry-run` performs all
of this reasoning without changing anything; it was verified to leave the preference file
byte-identical.

---

## 13. Logging

A persistent log is written to `logs/emergency-simulator.log` when running from the repository, or to
`%LOCALAPPDATA%\Emergency-Simulator\logs\` when packaged. It records the timestamp, device serial,
discovery result, root check, test-mode preparation, send attempt, injector result, downstream
evidence, and the final status. It does not record unrelated device data.

---

## 14. Troubleshooting

| Symptom | Cause and fix |
| --- | --- |
| `adb was not found` | Install Platform Tools and set `ADB_PATH`. See section 3. |
| `NO_DEVICE` | Nothing attached. Start an emulator or connect a device with USB debugging on. |
| `DEVICE_UNAUTHORIZED` | Accept the USB debugging prompt on the device. |
| `NO_ROOT` | Root is genuinely required. Use a userdebug/eng build or a rooted device. |
| `TEST_MODE_DISABLED` | The preferences could not be established; the message would be filtered. Check that root can write the receiver's prefs directory. |
| `INJECTOR_FAILURE` | The jar is missing or the push failed. See section 6. |
| `BROADCAST_REJECTED` | The sending context was not root (permission denial in logcat). |
| `CELLBROADCAST_FILTERED` | The receiver saw it but dropped it, usually `ignoring alert of type ... by user preference`: test mode is off. |
| `TIMEOUT` with no evidence | The device's CellBroadcast package may differ, or the receiver was force-stopped mid-run. Check `CellBroadcast: ...` in the device panel. |
| Alert shown twice | The alert queue accumulates. Only one send per explicit action; there is no retry. |
| Cannot dismiss the alert remotely | By design. Use the alert's own on-device control. |

---

## 15. Limitations

* Android 15 is verified. 14 and 16 are expected to work but are **not** experimentally confirmed.
* Root is required. Stock retail devices are out of scope.
* One device per send. Multi-device fan-out is not implemented.
* Lock-screen presentation, vibration and DND override are **not yet verified** (see
  [open-questions.md](open-questions.md)).
* Wi-Fi transport is not implemented; transport is USB/ADB.
* An already-displayed alert cannot be dismissed remotely.