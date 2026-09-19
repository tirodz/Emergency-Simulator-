# Emergency-Simulator-

An Android Cell Broadcast / Emergency Alert laboratory.

This repository is a **reverse-engineering and feasibility investigation**. It exists to answer one
question with evidence:

> Can a PC (or a controller phone) legitimately cause one or more Android test devices to process a
> controlled Cell Broadcast test message through Android's *genuine* emergency-alert subsystem —
> real alert classification, real sound, real vibration, real full-screen system UI — instead of a
> fake notification or a UI we reimplement ourselves?

The answer is **yes**, and it has been demonstrated. Android's own Cell Broadcast subsystem has
processed a controlled test message on a live Android 15 device and produced the real alert dialog,
real alert audio and real text-to-speech. See [`report.md`](report.md) and
[`docs/experiments.md`](docs/experiments.md).

This repository is the permanent technical record of the investigation: what is possible, on what
builds, with what privileges, on which OEMs, and what would require real cellular infrastructure.

## Status

**Phase: Windows release candidate — polished UI, self-contained EXE, and per-user installer are implemented.**

| | |
| --- | --- |
| Proven on | Android 15 / API 35, `userdebug`, AVD `test35` |
| Injection | `android/alertinject/` — a reflective `SmsCbMessage` builder run as root under `app_process` |
| Result | Real `CellBroadcastReceiver` → real `CellBroadcastAlertService` → real alert UI + sound + TTS |
| Requires | Root. Not an AOSP build, not a platform signature, not a system app. |
| Transmission | None. No modem, no radio, no network, at any point. |
| Controller | `tools/test_alert.py` (CLI) and the Tkinter GUI — one engine, two front ends |
| Windows app | `Emergency-Simulator.exe` — carries its own adb and injector, and verifies itself |
| Defects | [`docs/bugs/`](docs/bugs/) — every real defect, with the test that guards it |

## Quick start

The released executable needs nothing installed but Windows and a rooted device:

```powershell
# the device must be rooted and have USB debugging enabled
adb devices

# optional: see what the build resolved before touching anything
.\dist\Emergency-Simulator.exe --selftest=%TEMP%\selftest.txt

# then just run it
.\dist\Emergency-Simulator.exe
```

From a source checkout, Python 3.10+ and an adb somewhere are also needed:

```powershell
$env:ADB_PATH = "C:\platform-tools\adb.exe"
python tools\test_alert.py --list
python tools\test_alert.py --dry-run
python tools\test_alert.py --device emulator-5554
python app\main.py
```

The message must begin with `TEST`. The alert channel is fixed to the ETWS test channel `4355`
(0x1103) and cannot be changed. Build the executable with
`powershell -ExecutionPolicy Bypass -File packaging\build_windows.ps1`, or download the CI artifact
`Emergency-Simulator-windows`.

Full instructions, including how to build the injector and the executable, are in
[`docs/windows-controller.md`](docs/windows-controller.md).

## Windows release

Version **1.1.0** is the coordinated desktop build. GitHub Actions builds three deliverables: the self-contained `Emergency-Simulator.exe`, a GUI-only review build, and `Emergency-Simulator-Setup-1.1.0.exe`, a per-user Windows installer. The release workflow publishes those files to GitHub Releases when a `v*` tag is pushed.

The installed application still requires a controlled development target for actual injection. A stock retail Android phone is not presented as supported by this release.

## What this project is NOT

* It is **not** a way to transmit a real emergency broadcast over public cellular infrastructure.
* It is **not** an attempt to impersonate a carrier or government authority.
* It is **not** a fake-notification app. A custom notification is explicitly a *fallback
  simulation*, not the objective.

## Documentation map

| Document | Purpose |
| --- | --- |
| [`progress.md`](progress.md) | Live project state, findings, blockers, next actions |
| [`report.md`](report.md) | Consolidated technical investigation report |
| [`docs/windows-controller.md`](docs/windows-controller.md) | The desktop application: install, build, safety, CANCEL, troubleshooting |
| [`docs/architecture.md`](docs/architecture.md) | End-to-end architecture, verified against AOSP |
| [`docs/android-cellbroadcast.md`](docs/android-cellbroadcast.md) | What Cell Broadcast is and how Android models it |
| [`docs/aosp-test-path.md`](docs/aosp-test-path.md) | AOSP test application: exact call path and its boundaries |
| [`docs/protocol-and-alert-types.md`](docs/protocol-and-alert-types.md) | Message identifiers, ETWS/CMAS classes, the "rocket attack" question |
| [`docs/alert-experience.md`](docs/alert-experience.md) | What actually happens after the message arrives: sound, vibration, UI, DND |
| [`docs/privilege-model.md`](docs/privilege-model.md) | Permissions, AppOps, UID, signing, SELinux, build type |
| [`docs/android-version-compatibility.md`](docs/android-version-compatibility.md) | Android 14 / 15 / 16 matrix |
| [`docs/oem-compatibility.md`](docs/oem-compatibility.md) | Pixel, Samsung, Xiaomi, Motorola, Nothing |
| [`docs/transport-options.md`](docs/transport-options.md) | PC→device control channels (ADB, Wi-Fi, peer phone) |
| [`docs/feasibility.md`](docs/feasibility.md) | Feasibility matrix and answer to "can it be done" |
| [`docs/experiments.md`](docs/experiments.md) | Controlled experiment plan with objectives and expected results |
| [`docs/experiments/EXP-15.md`](docs/experiments/EXP-15.md) | The full end-to-end controller experiment, including the bugs found |
| [`docs/security-and-safety.md`](docs/security-and-safety.md) | Safety boundaries for the tool and for the controller |
| [`docs/open-questions.md`](docs/open-questions.md) | Explicit unknowns, no guesses |
| [`docs/critical-questions.md`](docs/critical-questions.md) | Direct answers to the project's 68 critical questions |
| [`docs/sources.md`](docs/sources.md) | Every AOSP repository, branch and commit inspected |

## Ground rules used while writing these documents

1. No invented Android APIs.
2. No claim is treated as true unless it is backed by AOSP source or official documentation. The
   exact repository and commit are recorded with each claim.
3. Unknowns are recorded as `UNKNOWN — requires experimental verification`, never filled with a
   guess.
4. If an approach is impossible, the reason is documented and the next approach is investigated.

<!-- original bootstrap content preserved below -->
