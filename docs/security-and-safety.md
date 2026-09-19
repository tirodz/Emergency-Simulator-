# Security and safety

This project deliberately touches a subsystem whose whole purpose is to make people stop what they
are doing. That imposes obligations that go beyond ordinary software hygiene.

## 1. Absolute safety rules

These are not negotiable and are not subject to a future session deciding otherwise.

1. **Never transmit on cellular spectrum.** The project must never include code that causes a radio to
   emit a Cell Broadcast. No SDR transmission, no base-station emulation, no modem-level injection.
2. **Never touch the modem path.** No `CbConfig` writes, no `MODIFY_CELL_BROADCASTS`, no channel
   programming on a live network. These APIs exist in the codebase; they are not needed for the test
   path and must stay unused.
3. **Never impersonate an authority.** No message may claim to come from a government, an emergency
   management agency, a carrier, or a named real event. This applies to the alert *body* text, which is
   free-form and therefore entirely our responsibility.
4. **Never send silently.** Every alert must be preceded by an explicit, human confirmation.
5. **Always label as a test.** Where the platform permits it (ETWS test warning type, CMAS monthly
   test / exercise / state-local test), use a genuine test category. Where the body text is under our
   control, prefix it with `TEST ALERT — SIMULATION / DEVELOPMENT`.
6. **Never weaken the platform's own safety behaviour.** Do not disable DND, bypass the notification
   system, suppress the dismiss button, or remove the platform's own warnings in order to make a demo
   more impressive.
7. **Do not use Presidential or AMBER classes for tests.** Presidential alerts cannot be turned off by
   the user; AMBER has a real-world meaning. Neither should ever be simulated.
8. **Keep the tool on a controlled network and controlled devices.** No deployment where a third party
   could be alarmed by an accidental alert.

## 2. Why an ordinary app cannot do this — and why that protects everyone

The security boundary documented in [`privilege-model.md`](privilege-model.md) is not an obstacle to
route around; it is the property that makes emergency alerts trustworthy. Specifically:

* `ACTION_SMS_EMERGENCY_CB_RECEIVED` and `SMS_CB_RECEIVED_ACTION` are `<protected-broadcast>` actions
  in `frameworks/base/core/res/AndroidManifest.xml`.
* `BroadcastController` refuses to deliver them unless the sender is a system UID or a persistent app.
* `RECEIVE_EMERGENCY_BROADCAST` is `signature｜privileged`.
* `CellBroadcastReceiver` performs **no caller check of its own** — it relies entirely on the
  broadcast mechanism.

Any tool that can inject emergency alerts therefore requires system-level access to the device. That
is exactly the correct design, and our project does not attempt to argue with it: we work *within* it,
on devices we control at the system level.

**Consequence for the architecture:** the difficulty of this project is a feature, not a bug. If it
were easy, so would be malicious use.

## 3. Safety design requirements for the controller

### 3.1 Confirmation flow

```
   [ SEND TEST ALERT ]
            |
            v
   +-----------------------------+
   | SEND TEST ALERT?            |
   | Target : Pixel-Test         |
   | Type   : ETWS test (0x1103) |
   | Message: TEST ALERT - ...   |
   | This is a SIMULATION.       |
   | [ CANCEL ]   [ SEND TEST ]  |
   +-----------------------------+
            |
            v
   (multi-device)
   +-----------------------------+
   | 3 DEVICES SELECTED          |
   | [ CANCEL ] [ SEND TO 3 ]    |
   +-----------------------------+
```

Requirements:

* Two-step: select, then confirm. Never one-click.
* The confirmation dialog must restate the target, the type, and the message.
* The word SIMULATION / TEST must appear in the dialog, not only in the message body.
* A "panic" / cancel must be reachable without navigating menus.

### 3.2 Arming limits

* Rate limit: reject a second alert within a configurable cooldown (suggested: 10 s) to prevent the
  tool being used as a way to spam a device's alarm.
* A maximum number of alerts per session, surfaced in the UI.
* An explicit "armed" state that must be enabled before sending, and that can be revoked.

### 3.3 Cancellation (the honest version)

Confirmed on a live device in EXP-ALERT-004. This is no longer a source-reading expectation.

| Control | Meaning | Supported |
| --- | --- | --- |
| CANCEL PENDING | drop a command that has not been executed on the device | yes |
| STOP SENDING | stop a multi-device fan-out mid-flight | yes |
| STOP ALERT AUDIO | end the sound by pressing the device's own dismiss control | yes, but only on-device |
| FORCE DISMISS SYSTEM ALERT | programmatically remove an already displayed alert | **NO — proven impossible** |

The last row must never be presented as available. It was tested two ways and both failed:

* `input keyevent 4` (BACK) did not dismiss the alert. The window manager log shows why: the dialog
  registers an `OnBackInvokedCallback` specifically so that BACK is swallowed.
* `am broadcast -a android.intent.action.CLOSE_SYSTEM_DIALOGS` did not dismiss it either.

Only the alert's own dismiss button works, and it is a visible action performed on the device. Any
controller must therefore expose "cancel before send" as a real capability and describe dismissal of
a delivered alert as a manual, on-device operator step — never as a remote kill switch.

One further safety-relevant behaviour: the dialog **queues** alerts. Two sends before the first was
acknowledged produced `OK (1/2)`. Retries therefore accumulate rather than replace, which is exactly
the kind of unintended alert amplification the cooldown and session-cap limits above exist to
prevent. Retry logic must confirm delivery before re-sending.

## 4. Security of the local controller

If the transport is Wi-Fi, the controller becomes an attack surface on the user's LAN. Requirements:

* **Bind to the LAN interface only.** Never `0.0.0.0` on a device with a routable address.
* **Pairing.** Devices are paired explicitly; pairing issues a random per-device token.
* **Authentication.** Every request carries the device token. Unpaired requests are rejected and
  logged.
* **Replay protection.** Monotonic counter or nonce per device; reject repeats.
* **Transport.** HTTPS where practical; on an isolated LAN, at minimum authenticate the body so a
  passive observer cannot forge a command.
* **No unauthenticated action endpoint.** Explicitly forbidden: `POST /emergency` reachable by anyone
  on the LAN.
* **Audit log.** Every command, its origin, its confirmation, and its result.
* **Rate limiting and lockout** on authentication failures.
* **Revocation.** A paired device can be unpaired, invalidating its token.

### Paired device view

```
Paired Devices

   ● TIRO-A35          READY
   ● Pixel-Test        READY
   ● Xiaomi-Lab        OFFLINE
```

Device state must always be visible, and "OFFLINE" must never silently become "sent".

## 5. Privacy

* Alert history on the device is written by Android's own `CellBroadcastContentProvider`. Our tool must
  not attempt to read device history without need, and must never exfiltrate it.
* The controller's logs must contain no personal data beyond device names the operator chose.
* No telemetry, no network calls to third parties. The tool must work fully offline
  (`transport-options.md` §6).

## 6. Threat model for the tool itself

| Threat | Mitigation |
| --- | --- |
| Accidental loud alert in a public place | confirmation dialog; test category; device selection |
| A bystander believing it is real | `TEST ALERT — SIMULATION` in the body; test category where possible |
| LAN attacker triggering alerts | pairing, tokens, replay protection, LAN-only binding |
| Alert spam against a test device | cooldown, session cap, revocation |
| Misuse of the injected text for impersonation | project rule §1.3; text templates with a mandatory test prefix |
| Rooted/custom device used for something else | out of scope; the tool grants no new capability beyond what root already implies |

## 7. What the project explicitly refuses to build

* A way to install the AOSP test APK on someone else's stock phone.
* A packaged "emergency alert sender" that works on unmodified devices.
* Any feature that hides the fact that an alert is a test.
* Any cellular transmission capability whatsoever.
* Anything that would let the controller act on a device the operator does not own and control.
