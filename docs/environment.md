# Environment assessment — EXP-ENV-001

**Mission:** 2A — build the Android/AOSP test environment.
**Date:** 2026-09-19
**Status:** CONFIRMED (environment characterised); emulator **AVAILABLE** under software emulation.

This document records what the OpenHands execution environment actually provides, established by
running the checks rather than assuming. It exists so a future session does not repeat them, and so
the hardware blockers are unambiguous.

---

## 1. Host platform

| Property | Value |
| --- | --- |
| Kernel | `Linux 6.8.0-1055-gke #61-Ubuntu SMP ... x86_64` |
| Distribution | `Debian GNU/Linux 13 (trixie)` |
| CPU | `AMD EPYC 9B14`, 2 cores/socket, 2 threads/core → **4 vCPU** |
| RAM | **15 GiB** total, 13 GiB available, **no swap** |
| Disk `/` | 72 GB overlay, 59 GB available |
| Disk `/workspace` | 25 GB volume, 25 GB available |
| Container | `systemd-detect-virt` → **`container-other`** |

## 2. Capability classification

| Capability | Verdict | Evidence |
| --- | --- | --- |
| `adb` | **AVAILABLE** | installed via Android SDK, `1.0.41` / `37.0.1-15733141` |
| `fastboot` | AVAILABLE | ships with `platform-tools` |
| Java / JDK | **AVAILABLE** | OpenJDK `21.0.12.1+1-1-deb13u1-Debian`. Note: Debian 13 ships **21**, not 17. |
| Android SDK | **AVAILABLE** | `/opt/android-sdk`, cmdline-tools `12.0` |
| Android Emulator | **AVAILABLE** | `37.1.11.0`, **boots without KVM** (see §4) |
| `repo` | MISSING | not installed; not needed for this mission |
| Git | AVAILABLE | `2.47.3` |
| Docker | AVAILABLE | `/usr/bin/docker`. **Cannot help**: containers do not gain KVM. |
| Python | AVAILABLE | `3.13.15` |
| Root in container | **AVAILABLE** | `sudo` grants full caps (`CapEff=000001ffffffffff`) |
| **KVM / hardware virtualization** | **BLOCKED** | see §3 |
| Nested virtualization | **BLOCKED** | no `vmx`/`svm` in `/proc/cpuinfo` |
| Build AOSP from source | **BLOCKED** | see §5 |

## 3. Virtualization — the decisive constraint

This is the single most important environment fact. It was tested rather than inferred:

```
grep -cE 'vmx|svm' /proc/cpuinfo        -> 0 occurrences
ls /dev/kvm                             -> No such file or directory
sudo mknod -m 666 /dev/kvm c 10 232     -> mknod: Operation not permitted
emulator -accel-check                   -> "KVM requires a CPU that supports vmx or svm"
systemd-detect-virt                     -> container-other
```

Interpretation:

* The host is a **container**, not a VM with passthrough.
* The CPU window exposed to us has **no `vmx`/`svm` bits**, so even if `/dev/kvm` existed, KVM could
  not function.
* `mknod` for the KVM device node is **denied even as root** — it is a cgroup/device-allowlist
  restriction, not a permissions problem we can fix.
* `kvm_intel` / `kvm_amd` kernel modules are **not loaded** and `modprobe` is unavailable.

**Conclusion: hardware-accelerated Android emulation is impossible in this environment, and cannot be
made possible from inside it.** This is a host-provisioning decision (the orchestrator must expose
`/dev/kvm` and the CPU virtualization flags), not a configuration we can change.

## 4. Software emulation works — the mitigation

Although KVM is blocked, the Android emulator **does boot under pure software emulation (QEMU TCG)**.
This was verified end to end:

```
emulator -avd test35 -no-window -no-audio -no-boot-anim \
         -gpu swiftshader_indirect -accel off -no-snapshot -memory 2048
```

* Boot to `sys.boot_completed=1` took **≈9 minutes** (t=540 s at 20 s poll intervals).
* `adb devices` → `emulator-5554  device`.
* `adb root` succeeds (build is `userdebug`).

Costs and caveats:

* Roughly **one CPU core is saturated** for the whole boot and during operation.
* Only **2 emulated cores** were configured; more would be slower, not faster.
* Boot time is minutes, not seconds. Any future workflow must budget for this and should prefer
  **snapshot save/load** over repeated cold boots.
* The system image is x86_64, so no CPU emulation penalty on top of the KVM penalty — it is still
  TCG-interpreted, hence the slow boot.

**This is the environment the missions should use.** It satisfies the mission's preferred Option A/B
requirement: an AOSP-family **userdebug** build with `adb`, `logcat`, and the Cell Broadcast
components present.

## 5. AOSP source build feasibility

Estimated requirements for a modern AOSP tree (`repo sync` + `m`):

| Resource | Requirement | Available | Verdict |
| --- | --- | --- | --- |
| Disk, source tree | ~100–250 GB | 59 GB `/` + 25 GB `/workspace` | **INSUFFICIENT** |
| Disk, build output | ~50–150 GB additional | — | **INSUFFICIENT** |
| RAM | 32–64 GB recommended (16 GB minimum with heavy swap) | 15 GB, **no swap** | **INSUFFICIENT** |
| CPU | 8–16+ cores for sane build times | 4 vCPU | **MARGINAL / impractical** |
| Time | 3–10 h on a strong machine | — | **impractical** |

**Verdict: BLOCKED. Do not attempt a full AOSP build here.** A tree plus output exceeds available
disk several times over, and the Soong/Ninja build would thrash a 15 GB no-swap machine with 4 vCPUs
for many hours. This is not a marginal call; it is not viable.

Consequence for the mission: we can run a **prebuilt** Android image, but we cannot build a custom one
here. Anything requiring a platform-signed APK built by us must happen on another machine — which is
exactly how the AOSP test-app route is gated (see `docs/device-cellbroadcast.md`).

## 6. Toolchain actually installed

Reproducible commands are in [`environment-setup.md`](environment-setup.md). Summary:

| Component | Version | Path |
| --- | --- | --- |
| OpenJDK | 21.0.12.1 | `/usr/lib/jvm/java-21-openjdk-amd64` |
| cmdline-tools | 12.0 | `/opt/android-sdk/cmdline-tools/latest` |
| platform-tools (adb) | 37.0.1 | `/opt/android-sdk/platform-tools` |
| emulator | 37.1.11.0 | `/opt/android-sdk/emulator` |
| platforms | android-35 | `/opt/android-sdk/platforms` |
| system image | `android-35;google_apis;x86_64` | `/opt/android-sdk/system-images/...` |

## 7. What this environment can and cannot prove

**Can prove:**

* That the production Cell Broadcast components exist and are reachable on a userdebug build.
* That `adb root` is granted and what identity it yields.
* Whether a root-identity broadcast reaches `CellBroadcastReceiver.onReceive`.
* The behaviour of the alert pipeline's entry conditions (`shouldDisplayMessage`, channel ranges,
  `isEmergencyMessage`, full-screen decision).
* The exact data format the receiver expects (`SmsCbMessage` under extra key `message`) — by source
  trace, and by attempted construction.

**Cannot prove:**

* Anything requiring KVM-level performance (not needed for correctness).
* Anything requiring a locally built, platform-signed APK.
* Real modem/CBS decode behaviour.

**Must be done elsewhere:** building `CellBroadcastReceiverTests`, and any run against physical OEM
hardware.