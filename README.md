# Emergency-Simulator-

An Android Cell Broadcast / Emergency Alert laboratory.

This repository is a **reverse-engineering and feasibility investigation**. It exists to answer one
question with evidence:

> Can a PC (or a controller phone) legitimately cause one or more Android test devices to process a
> controlled Cell Broadcast test message through Android's *genuine* emergency-alert subsystem —
> real alert classification, real sound, real vibration, real full-screen system UI — instead of a
> fake notification or a UI we reimplement ourselves?

The answer is **not yet committed to**. This repository is the permanent technical record of the
investigation: what is possible, on what builds, with what privileges, on which OEMs, and what would
require real cellular infrastructure.

## Status

**Phase: research / feasibility.** No application code is being written yet. See
[`progress.md`](progress.md) for the live state and [`report.md`](report.md) for the consolidated
findings.

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
