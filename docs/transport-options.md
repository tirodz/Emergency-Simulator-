# Transport options: PC → device control

This document is about **how the controller reaches the device**. It is deliberately independent of
the privilege question, because the answer to "how do the bytes get to the phone" is much easier than
"is the phone allowed to act on them". Do not let transport success be mistaken for feasibility.

## 1. The critical distinction

```
   CONTROLLER  ----transport---->  DEVICE AGENT  ----privileged call---->  genuine alert
                                        ^
                                        |
                            this is the hard part
```

A transport only moves a command. It does **not** confer privilege. Whatever privilege analysis
applies to the device agent (`privilege-model.md`) applies unchanged whether the agent is driven by
USB, Wi-Fi, or another phone.

Therefore: **choose the transport for operability, not for capability.**

## 2. Transport comparison

| Transport | Setup cost | Device discovery | Multi-device | Security posture | Verdict |
| --- | --- | --- | --- | --- | --- |
| USB + ADB | low (dev options + USB debugging) | `adb devices` | manual, port juggling | host-trusted; ADB keys are per-host | **Recommended for the first PoC** |
| Wi-Fi + TCP/HTTP | medium (agent must be installed and running) | mDNS / manual IP | natural | needs pairing + auth; must be LAN-scoped | Recommended for phase 2 |
| Wi-Fi + WebSocket | medium | mDNS | natural, push-capable | same as HTTP; better for CANCEL/ack | Good for delivery status |
| Wi-Fi + UDP broadcast | low | implicit | natural | **weak** — spoofable, no auth; acceptable only on a trusted isolated LAN | Not for alert-relevant commands without a signature layer |
| Second Android phone as controller | medium | mDNS / QR | natural | same as Wi-Fi | Feasible; means phone↔phone, not phone→cell |
| Bluetooth | medium | pairing | limited | reasonable | Not recommended; pairing friction, short range |
| NFC | trivial per-tap | implicit | poor | physical proximity is a nice safety property | Interesting for "arm" gestures only |

## 3. USB + ADB (first PoC)

### Why it is the right first step

* No network discovery problem: `adb devices -l` enumerates serials, models and state.
* No pairing protocol to design: ADB's own key trust is the auth.
* Deterministic failure modes: if `adb shell` is refused, nothing happens; there is no silent
  half-delivery.
* It maps naturally onto "one developer machine, one test device".

### What ADB can and cannot do here

From `privilege-model.md`, Gate 1 (protected broadcast / system UID) blocks `shell` from sending the CB
broadcast directly. So an ADB-only design must be one of:

* **A device-side agent** already present with the needed privilege, which ADB *invokes* rather than
  acts as; or
* **A privileged command channel** that we install (root / custom build); or
* nothing — if neither exists, ADB alone is insufficient.

ADB is a *remote control*, not an authority.

### Confirmed by Experiment 3: the AOSP test app is not an option on a shipping device

The obvious hope was that ADB could simply drive the existing AOSP test app. Experiment 3 established
that this fails at the first step, for two independent reasons:

1. **The test APK ships on no build type.** It is absent from a real AOSP GSI and from any
   `PRODUCT_PACKAGES`. There is nothing on a stock or already-flashed device for ADB to launch.
2. **The activity is not Intent-drivable.** `SendTestBroadcastActivity` has no `onNewIntent` and
   never reads its Intent. `am start` opens the UI and sends nothing. A UI tap is mandatory.

So the "ADB → exported test Activity → genuine alert" chain is real in principle but **cannot be
completed by ADB alone on a stock device**. ADB's role is reduced to:

* launching the test UI (`am start`), and
* injecting the tap that actually invokes the send callback (`input tap` / `uiautomator`).

Both steps are only possible once the platform-signed test APK exists on the device, which requires a
custom AOSP build or a rooted device.

### The honest shape of the phase-1 PoC

```
precondition : device running a userdebug/eng AOSP build, or a rooted device
             : with the platform-signed CellBroadcastReceiverTests APK present

PC                                      Device
script                                  adbd
  |                                        |
  | adb -s <serial> shell am start \        |   (1) render the test UI
  |   -n com.android.cellbroadcastreceiver.tests/.SendTestBroadcastActivity
  +--------------------------------------->|
                                           |
  | adb -s <serial> shell input tap X Y     |   (2) press "ETWS test" (coords from uiautomator)
  +--------------------------------------->|
                                           v
                                   SendTestMessages.testSendEtwsMessageTest(...)
                                   (runs as android.uid.phone, platform-signed)
                                           |
                                           v
                                   sendOrderedBroadcastAsUser(
                                     ACTION_SMS_EMERGENCY_CB_RECEIVED,
                                     package = com.android.cellbroadcastreceiver,
                                     receiverPermission = RECEIVE_EMERGENCY_BROADCAST,
                                     appOp = OP_RECEIVE_EMERGECY_SMS)
                                           |
                                           v
                                   genuine CellBroadcastReceiver.onReceive
                                           |
                                           v
                                   genuine alert UI + sound + vibration
```

The controller is therefore a *UI driver* plus a *logcat observer*, not a broadcaster. That is a
legitimate but more fragile design than originally hoped, because it depends on screen layout.
`uiautomator dump` should be used to resolve coordinates at run time rather than hardcoding them.


### Sketch

```
PC                                      Device
python client                           adb daemon (adbd)
  |                                        |
  | adb -s <serial> shell <agent-cmd>       |
  +--------------------------------------->|
                                           v
                                   device-side agent
                                   (runs as system/phone uid)
                                           |
                                           v
                                   sendOrderedBroadcast(...)
                                           |
                                           v
                                   genuine alert
```

### Operational notes

* Use `adb -s <serial>` everywhere; never rely on a single attached device.
* `adb devices` states: `device`, `offline`, `unauthorized`. An `unauthorized` device must not be
  treated as ready (this is a controller-state problem, not an Android problem).
* "Airplane mode" does not block this path, because the path does not use the radio.
* "No SIM" does not block the broadcast path either, **but** it may affect the receiver's readiness
  (`CellBroadcastReceiver` handles `ACTION_SERVICE_STATE` and `DEFAULT_SMS_SUBSCRIPTION_CHANGED`;
  some channel configuration is driven by subscription state). Whether an alert displays with no SIM
  is an **UNKNOWN** to verify. -> Experiment 11.

## 4. Local Wi-Fi (phase 2)

### Discovery

* **mDNS / DNS-SD** (`_cbsim._tcp.local`) is the natural fit if the agent advertises itself.
* Manual IP entry is a perfectly acceptable fallback and avoids a mDNS implementation in the agent.
* QR-code pairing (`cbsim://<ip>:<port>?token=...`) solves both discovery and first-trust in one step.

### Command surface (proposed, not implemented)

```
GET  /v1/health                 -> {device, model, build, agentVersion, privileges}
GET  /v1/status                 -> ready/offline, last alert, pending
POST /v1/test-alert             -> {type, channel, message, serial}   (requires pairing + confirm)
POST /v1/cancel-pending         -> cancel BEFORE delivery
POST /v1/dismiss                -> explicitly documented as unsupported on the system UI
```

Note the deliberate asymmetry: `/cancel-pending` exists, `/dismiss` does not promise anything.
See §7 below.

### Security requirements (non-negotiable)

* Bind to the LAN interface only; never `0.0.0.0` on a device with a public interface.
* Require a per-device random token issued at pairing; store it, do not derive it from a fixed secret.
* Reject unsigned/unpaired requests with a clear error; log every attempt.
* HTTPS where practical; on a trusted isolated LAN, at minimum HMAC the request body with the paired
  token so a passive replay cannot be forged.
* Include a monotonically increasing counter/nonce per device for replay protection.
* Require an explicit confirmation step server-side; do not let a stray LAN request cause an alert.
* Rate-limit: an emergency-alert simulator that can be invoked in a loop is a denial-of-service tool
  against the user's own attention.

Full discussion in [`security-and-safety.md`](security-and-safety.md).

## 5. Controller phone (phone A → phone B)

### What it is

Phone A is a **client** of the same command surface. It is not, and must never become, a cellular
transmitter. It sends an HTTP/mDNS command to phone B, exactly as the PC would.

```
   Phone A (controller)                    Phone B (target)
   - mDNS browse / QR scan                 - agent advertises
   - paired token                          - paired token
   - POST /v1/test-alert ----------------> - device-side agent
                                             |
                                             v
                                           genuine alert (sound/vibrate/UI)
```

### Feasibility

Feasible **provided the device agent exists**. Phone A needs only network permission and a client
implementation; the privilege burden is entirely on phone B, unchanged from the PC case.

### Extra safety property

Because the controller is itself an Android device with a screen, it can show the same confirmation
dialog and the same "SIMULATION / DEVELOPMENT" labelling as the PC controller. That is desirable: the
confirmation should live wherever the finger is.

## 6. No-Internet requirement

The entire design must work with **no internet connection**. Check:

| Component | Needs internet? |
| --- | --- |
| ADB over USB | no |
| mDNS discovery | no (link-local multicast) |
| HTTP/WebSocket on LAN | no |
| The alert injection itself | no |
| Genuine alert UI/audio/vibration | no |

This is a structural safety property: a system that cannot reach the internet cannot accidentally
cause a real broadcast through a network API.

## 7. CANCEL semantics as a transport concern

Two different operations, which must not be conflated:

| Operation | Meaning | Transport |
| --- | --- | --- |
| Cancel pending | the command has not yet been executed on the device; drop it | device agent - yes |
| Dismiss displayed alert | the system alert UI is already up; make it go away | **not supported by any transport** unless the receiver itself provides a path |

Research on the second item: `CellBroadcastAlertService` exposes `DISMISS_DIALOG`, and the dialog has a
`dismiss()` method that stops `CellBroadcastAlertAudio` and removes the notification. But these are
internal to the privileged CBR app. There is no exported entry point found in the sources inspected so
far for a third party to dismiss an alert. -> Open question and Experiment 9.

The recommended controller UX therefore is:

```
[ SEND TEST ALERT ]        -> arm + confirm + send
[ CANCEL PENDING ]         -> valid only before the device reports "injected"
[ CLOSE ON DEVICE ]        -> instructs the operator to dismiss on the device; NOT an automatic
                              remote dismissal
```

## 8. Reliability requirements for multi-device

Design constraints to keep the system from being fragile:

* Every command carries a unique id; the agent echoes it back in the result.
* Device states are explicit and visible: `READY`, `BUSY`, `OFFLINE`, `UNPAIRED`, `UNSUPPORTED`.
* Send is per-device fan-out with per-device acknowledgement; a global "sent" must never be reported
  when only some devices acknowledged.
* Retries are idempotent — the same command id must not produce a second alert.
* Duplicate detection is *both* transport-level (command id) and Android-level (serial number; CBR
  dedups on its history database).
* Timeouts are surfaced as `UNKNOWN`, never as success.

Target scale: 3–10 devices on one LAN, which is well within the capability of a trivial HTTP/WebSocket
fan-out. No performance engineering is warranted before the mechanism itself is proven.
