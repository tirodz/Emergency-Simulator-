# Answers to the 68 critical questions

Direct answers. Where the answer is not established, it is marked
`UNKNOWN — requires experimental verification` with the experiment number, and it is not guessed.

## Fundamental

**1. What exactly is a Cell Broadcast?**
A point-to-multipoint message service of GSM/UMTS/LTE/5G. One message is transmitted to every device
on a cell; there is no per-subscriber addressing. Android receives the raw PDU from the radio layer.

**2. What exactly does Android receive?**
A raw PDU (`byte[]`) over `ICellBroadcastService.handleGsmCellBroadcastSms` /
`handleCdmaCellBroadcastSms`, which CBS decodes into an `SmsCbMessage`. The framework itself never
decodes the PDU.

**3. What component receives it?**
`com.android.cellbroadcastservice` (CBS) at the radio boundary; then, at the app-visible boundary, the
privileged `com.android.cellbroadcastreceiver` (CBR).

**4. What component displays the alert?**
`com.android.cellbroadcastreceiver` — specifically `CellBroadcastAlertDialog` (activity) and
`CellBroadcastAlertService`.

**5. Where does emergency audio originate?**
`CellBroadcastAlertAudio` (a service in CBR), playing `res/raw/*.ogg` on `USAGE_ALARM` /
`STREAM_ALARM`, with the alarm stream volume forced to maximum.

**6. Where does vibration originate?**
`CellBroadcastAlertAudio` via `Vibrator.vibrate(VibrationEffect, AudioAttributes)`.

**7. Where does the full-screen UI originate?**
`CellBroadcastAlertDialog`, using `FLAG_FULLSCREEN | FLAG_SHOW_WHEN_LOCKED | FLAG_TURN_SCREEN_ON |
FLAG_KEEP_SCREEN_ON`.

## Testing

**8. Does AOSP provide a Cell Broadcast test application?**
Yes. `packages/apps/CellBroadcastReceiver/tests/testapp/`, package
`com.android.cellbroadcastreceiver.tests`.

**9. How does it construct messages?**
Two ways: the CMAS path constructs `SmsCbMessage` directly; the ETWS/generic path builds a raw hex PDU,
patches the serial number and identifier, and decodes it with its own copy of `GsmSmsCbMessage`.

**10. What permissions does it need?**
Declares `BROADCAST_SMS` and `INTERACT_ACROSS_USERS_FULL`; actually relies on
`android:sharedUserId="android.uid.phone"` plus the platform signing certificate, and passes
`RECEIVE_EMERGENCY_BROADCAST` as the broadcast's `receiverPermission`.

**11. Can we use the same mechanism on Android 16?**
Yes. `android16-release` (`b97c8a4f…`) carries the same mechanism, unchanged from 14 and 15.

**12. Does it invoke the genuine alert UI?**
Yes. It enters at `CellBroadcastReceiver.onReceive`, which is the genuine entry point; the dialog that
follows is the production `CellBroadcastAlertDialog`.

**13. Does it invoke genuine alert sound?**
Yes — `CellBroadcastAlertAudio` with the production tone for the chosen class.
`UNKNOWN — Experiment 5` for the observable result on hardware.

**14. Does it invoke vibration?**
Yes by code path (`Vibrator.vibrate(...)` in `CellBroadcastAlertAudio`).
`UNKNOWN — Experiment 11` for behaviour in vibrate-only/ringer-mode states.

**15. Does it wake the screen?**
The dialog requests `FLAG_TURN_SCREEN_ON`. `UNKNOWN — Experiment 11` for the observable result.

**16. Does it work on a locked device?**
The dialog requests `FLAG_SHOW_WHEN_LOCKED`. `UNKNOWN — Experiment 11` for the observable result.

## Security

**17. Can an ordinary APK do this?**
**No.** Four independent reasons: protected broadcast (system UID check in `BroadcastController`),
`RECEIVE_EMERGENCY_BROADCAST` is `signature|privileged`, no `android.uid.phone`, no platform signing.

**18. Can ADB do this?**
Not by `am broadcast` of the protected action. ADB *might* work by `am start` of the test app's own
exported activity. `UNKNOWN — Experiment 6 (highest priority)`.

**19. Can shell do this?**
**No.** Shell is UID 2000, which is not in `BroadcastController`'s accepted list, and no shell command
interface for injecting CB was found in either CB module.

**20. Can root do this?**
Root is `ROOT_UID`, which the protected-broadcast check explicitly accepts.
**LIKELY** — AppOps and SELinux remain unverified. `UNKNOWN — Experiment 7`.

**21. Can a privileged APK do this?**
Only if it *also* runs as a system UID or holds a matching identity. Privileged status alone does not
satisfy `BroadcastController`.

**22. Is system signing required?**
Practically, yes: the identity that satisfies the UID check is obtained in practice by platform signing
plus a shared system UID. The AOSP test app uses both.

**23. Is userdebug required?**
**No** for the permission model — the check is build-type independent. userdebug is required for the
*covenient* extras (installing a platform-signed APK, `debug_build` channels, the CBS extra test
broadcast).

**24. Is eng required?**
No. Nothing in the path requires eng specifically.

**25. Does SELinux matter?**
`UNKNOWN — Experiment 7`. No SELinux policy has been inspected. It may further restrict a hand-rolled
helper.

## Hardware

**26. Does the modem need to participate?**
**No** for the user-visible result. Nothing in classification, settings, database, sound, vibration or
UI reads radio state.

**27. Can this happen entirely above the modem?**
**Yes.** That is the central finding of this investigation.

**28. Does the test path differ from a real cellular broadcast?**
**Yes, in exactly one place.** It enters at the ordered-broadcast boundary that separates CBS from CBR,
skipping only the modem and the CBS decode step. Everything downstream is production code.

**29. Can an emulator participate?**
`UNKNOWN — Experiment 3`. It depends on whether the image contains the CB apex and the receiver.

**30. Can a development phone participate?**
**Yes**, and it is the recommended path: a userdebug/eng AOSP build, a GSI, or a custom ROM.

## Network

**31. Can a PC trigger it?**
**Yes**, via a device-side agent holding the required identity, or via ADB if Experiment 6 succeeds.

**32. Can USB/ADB trigger it?**
USB/ADB is the recommended first transport. Whether it can trigger the alert *without* a custom agent
is `UNKNOWN — Experiment 6`.

**33. Can Wi-Fi trigger it?**
Yes, once a device-side agent exists. Design in `transport-options.md`.

**34. Can a second Android phone trigger it?**
Yes on the same terms as a PC — it is a controller, not a transmitter. It needs no cellular capability.

**35. Can multiple Android phones be controlled?**
Yes. Per-device fan-out with acknowledgement and explicit state. Architecturally trivial; not yet
implemented.

**36. Can the system work without Internet?**
**Yes, confirmed by construction.** ADB is local, mDNS is link-local, the injection path touches no
network, and the alert is local. A fully offline system cannot reach a cellular network API, which is a
structural safety property.

## Cancellation

**37. Can a pending alert be cancelled?**
Yes, trivially — a command not yet executed is under our control.

**38. Can an already displayed emergency alert be dismissed remotely?**
No exported interface was found. `DISMISS_DIALOG` and `CellBroadcastAlertDialog.dismiss()` are internal
to the privileged CBR app. `UNKNOWN, leaning NO — Experiment 9`.

**39. What can Android deliberately prevent us from doing?**
Remote dismissal of a displayed alert; overriding DND for test-class alerts; anything that would let a
non-system caller inject an emergency alert. These are security properties, not defects.

## Compatibility

**40. Android 14?** Mechanism present and unchanged (`346bb742…`). `CONFIRMED` from source.
**41. Android 15?** Same (`62e355af…`). `CONFIRMED` from source.
**42. Android 16?** Primary reference (`b97c8a4f…`); app became `updatable: true`. `CONFIRMED` from source.
**43. Pixel?** `UNKNOWN — Experiment 10`. Recommended reference OEM.
**44. Samsung?** `UNKNOWN — Experiment 12`. Whether Samsung ships the AOSP receiver is unverified.
**45. Xiaomi?** `UNKNOWN — Experiment 13`.
**46. Motorola?** `UNKNOWN — Experiment 14`.
**47. Nothing?** `UNKNOWN — Experiment 14`.
**48. Stock devices?** Injection **NOT POSSIBLE**; observation and configuration only.
**49. Rooted devices?** **LIKELY**, pending Experiments 7 and 9.
**50. Development builds?** **CONFIRMED** by construction from AOSP source; hardware confirmation pending Experiment 5.

## Protocol

**51. What are ETWS?** Earthquake and Tsunami Warning System. Earthquake/tsunami warnings for Japan.
Carries a warning type inside the message; identifiers `0x1100`–`0x1104`.

**52. What are CMAS?** Commercial Mobile Alert Service — US Wireless Emergency Alerts. Identifiers
`0x1112`–`0x111E` plus language variants `0x111F`–`0x112B`. Carries class, category, severity, urgency,
certainty and response type.

**53. What is the difference?** ETWS is a Japanese earthquake/tsunami system with a warning-type field
and its own tones. CMAS is the US multi-hazard system with a richer classification and a class-driven
title. Android recognises both, on different identifier ranges and with different toggles.

**54. What does Android recognise?** All of the identifiers listed in `protocol-and-alert-types.md` §2
and §3, quoted from `SmsCbConstants`. Anything outside a configured channel range is silently dropped.

**55. Which test categories are available?** ETWS test (`0x1103`), CMAS monthly test (`0x111C`), CMAS
exercise (`0x111D`), CMAS operator-defined (`0x111E`), state/local test (overlay-defined), and
carrier/vendor test channels where an overlay defines them.

**56. Can the message text be customized?** **Yes.** `SmsCbMessage.getMessageBody()` is free-form. The
*title* is Android's own and is overlayable by OEMs.

**57. Can we make a test message visually resemble a real emergency alert while remaining a test?**
Yes — use a genuine test class so Android labels it a test, and use the CMAS display fields (category,
severity, urgency, certainty, response type) so Android renders its own rich emergency layout. The
body text must still identify the alert as a test.

**58. What fields are mandatory?** Message format, geographical scope, serial number, location, service
category, language code, a **non-empty message body**, priority, and the ETWS or CMAS info object. See
`protocol-and-alert-types.md` §6.

## Project

**59. What is the smallest possible proof of concept?** PC → `adb` → the AOSP test app's own activity
on a userdebug/eng device → one ETWS test message → genuine alert. If Experiment 6 succeeds, an
ADB-only variant with no custom agent.

**60. What is the minimum hardware required?** One Android device capable of running a userdebug/eng
build (or a GSI), one PC, one USB cable.

**61. What is the minimum software required?** A matching AOSP checkout to build and install
`CellBroadcastReceiverTests`, and an ADB client.

**62. What should the first working demo look like?** Terminal script, two commands: send one
ETWS test alert, and cancel a pending one. Proof is logcat showing the production components plus the
row in Android's own CB history.

**63. What should the final architecture look like?** PC controller → authenticated local network →
per-device privileged agent → `CellBroadcastReceiver.onReceive` → genuine alert. Diagram in
`report.md`.

**64. What cannot be achieved?** Injection on a stock, unrooted device. Remote dismissal of a displayed
alert. Real broadcast without cellular infrastructure. DND override for test-class alerts.

**65. What requires a custom Android build?** Nothing in the permission model. The conveniences do:
installing the platform-signed test APK, `debug_build` channels, and the clean end-to-end proof.

**66. What requires root?** Injecting the protected broadcast from a non-platform process on a retail
build; installing a durable privileged helper outside the system image.

**67. What requires actual cellular equipment?** Only a *real* Cell Broadcast. Everything the project
wants does not.

**68. What should we NOT attempt?** Any cellular transmission. Impersonating an authority. Silent or
one-click alerts. A fake emergency UI passed off as success. Presidential/AMBER classes for tests.
Weakening Android's own safety behaviour. Root on a device the operator does not own.
