# Open questions

Every entry here is an explicit unknown. None have been guessed. Each maps to an experiment in
[`experiments.md`](experiments.md).

Format: **question** — status — next action.

## A. Privilege and injection

1. **Does a *system* caller still need the `receiverPermission` argument?**
   `BroadcastController` performs the protected-broadcast UID check first, and the AOSP test app both
   holds the UID *and* passes the permission.
   — Status: **ANSWERED — the UID check alone is sufficient.** Root sent the broadcast with no
     manifest permission, no AppOps grant and no SELinux change, and the receiver processed it
     normally (EXP-ALERT-002).

2. **Is there any existing system component with an exported interface that can inject a CB message?**
   No shell command service, no binder injection API was found in `CellBroadcastService` or
   `CellBroadcastReceiver`. Whether an OEM or a vendor component adds one is unchecked.
   — Status: UNKNOWN (AOSP: none found) — Experiments 6, 12.

3. **Does AppOps block a rooted helper that holds the permission?**
   — Status: **ANSWERED — no.** AppOps was never consulted on this path. See EXP-ALERT-002.

4. **Does SELinux deny a hand-rolled helper that is not part of the platform?**
   — Status: **ANSWERED — no.** A non-platform process running as root under `app_process` produced
     the genuine alert without any policy change. See EXP-ALERT-002.

5. **Exactly which SELinux domains are involved** for sending the protected broadcast and writing the
   CB history provider? — Status: UNKNOWN — Experiment 7.

6. **Can Magisk (or an equivalent) install a durable privileged helper that survives updates?** —
   Status: UNKNOWN — not yet investigated.

7. **Does `am start` on the AOSP test activity work from ADB and produce a genuine alert?**
   This is the highest-value unknown, because a positive answer makes an ADB-only controller viable.
   — Status: UNKNOWN — Experiment 6.

## B. Build and environment

8. **Does an AOSP emulator or GSI image contain the CellBroadcast apex and the CB receiver?**
   — Status: **ANSWERED — yes, on both.** Present in a Google GSI (Experiment 3) and on the running
     `android-35;google_apis;x86_64` target (EXP-ENV-002).

9. **Does a GSI include the CB test app?**
   — Status: **ANSWERED — no, on no build type.** Confirmed by image walk and on the live device.

10. **Does the platform signing key available when building a custom AOSP image match the key the
    pre-installed CBR app is signed with?**
    — Status: **MOOT.** The test APK is not needed; nothing has to be installed as a system app.
      See `aosp-test-path.md` §10.

11. **Which Android 16 QPRs / feature flags change the CBR entry point?** The CBR repo contains a
    `flags/` directory that has not been inspected. — Status: UNKNOWN — source inspection needed.

## C. Alert behaviour

12. **Does the injected alert display with no SIM present?**
    — Status: **ANSWERED — YES.** The emulator has no active cellular subscription and the full alert
      experience occurred, on `subId 0`. Channel configuration is not SIM-dependent for this path.
      See EXP-ALERT-002.

13. **Does the alert display in airplane mode?** — Status: UNKNOWN — Experiment 11.

14. **How does alert audio route with Bluetooth or wired headphones connected?**
    `CellBroadcastAlertAudio` forces `STREAM_ALARM` to maximum volume, but routing is not examined.
    — Status: UNKNOWN — Experiment 11.

15. **Does DND get overridden for a test channel?** Depends on the channel's `override_dnd` and the
    global `override_dnd` setting, and on `AudioAttributes.FLAG_BYPASS_INTERRUPTION_POLICY`, which is
    only effective for `USAGE_ALARM`.
    — Status: PARTIALLY UNDERSTOOD from source; behaviour unverified — Experiment 11.

16. **Is `FLAG_TURN_SCREEN_ON` removed if the screen turns off while the dialog is displayed?**
    `CellBroadcastAlertDialog` has an `onScreenOff` handler that clears `FLAG_TURN_SCREEN_ON`.
    — Status: understood from source; user-visible effect unverified — Experiment 11.

17. **What is the exact dismissal UX** for a multi-message alert?
    — Status: **PARTIALLY ANSWERED.** The dialog shows `OK (1/2)` for two queued messages, and
      dismissing advances the queue rather than clearing it. Sends accumulate; they do not replace
      the current alert. See EXP-ALERT-004.

18. **Can an already displayed alert be dismissed remotely by any means?**
    — Status: **ANSWERED — NO.** Tested on the device: BACK and
    `android.intent.action.CLOSE_SYSTEM_DIALOGS` are both ignored, and the window manager log shows
    the dialog registering an `OnBackInvokedCallback` specifically to swallow BACK. Only the alert's
    own dismiss button works, and it is a visible on-device action. See EXP-ALERT-004 and
    checkpoint 3.

## D. OEM behaviour

19. **Does Samsung ship the AOSP `CellBroadcastReceiver` or replace it?** — Status: UNKNOWN —
    Experiment 12.

20. **Does Samsung/Toolbar/SystemUI intercept the emergency dialog?** — Status: UNKNOWN —
    Experiment 12.

21. **Does Xiaomi/HyperOS ship an AOSP-compatible receiver?** — Status: UNKNOWN — Experiment 13.

22. **Is `allow_testing_mode_on_user_build` `false` on any OEM retail build?** Known to be `false` in
    the AOSP Japan/docomo overlay; OEM behaviour unknown.
    — Status: UNKNOWN — Experiment 2 per OEM.

23. **Do Pixel retail builds overlay the alert wording or channel ranges in a way that blocks the test
    channels?** — Status: UNKNOWN — Experiment 10.

24. **Which OEMs define `state_local_test_alert_range_strings`?** Empty in AOSP defaults, so
    state/local test alerts are dropped on bare AOSP. — Status: UNKNOWN per OEM.

## E. Protocol

25. **Which ETWS `warningType` does Android assign to `0x1103` when decoded by CBS rather than by the
    test app's own decoder?** The test app sets it explicitly; the production decoder path was not
    traced line by line.
    — Status: UNKNOWN — source inspection needed.

26. **How does CBR's `getCellBroadcastChannelResourcesKey` behave for a channel that matches no range
    but IS inside the PWS range 4352–6399?** `isChannelEnabled` may return `true` via the ETWS branch
    or fall through, while `handleCellBroadcastIntent` still drops it because `range == null`.
    — Status: needs careful source reading and a device test — Experiment 5.

27. **Do carrier overlays on real devices define additional `testing_mode=true` channels we should
    prefer?** Known examples in UK/Bulgaria/Korea overlays. — Status: partially known — Experiment 2.

## F. Transport and controller

28. **Is mDNS discovery reliable across Android's multicast filtering?** Some devices aggressively
    filter multicast in doze. — Status: UNKNOWN — phase 2 experiment.

29. **Will an Android device accept a self-signed HTTPS certificate from a peer device without a
    trust-store change?** — Status: UNKNOWN — phase 2 experiment.

30. **What is the right way to make the controller's confirmation survive a device that is offline
    mid-fan-out?** — Design question, not a factual unknown.

## G. Explicitly out of scope but recorded

31. **What would a genuine cellular Cell Broadcast test require?** A CBC, RAN equipment, core-network
    configuration, spectrum and authorization. Recorded in `feasibility.md` §5 as *not required* for
    this project. No further investigation planned, deliberately.
