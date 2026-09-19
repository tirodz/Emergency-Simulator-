# OEM compatibility

**This document is deliberately mostly empty of conclusions.** OEM behaviour has not been inspected
in source. Only one OEM overlay was found in AOSP, and it is documented below. Everything else is
recorded as an unknown with a planned experiment.

## 1. What AOSP gives us: the overlay mechanism

The CellBroadcastReceiver app is built so that OEMs override its behaviour through resource overlays
(RRO), not by forking the app. Evidence: `packages/apps/CellBroadcastReceiver/res/values/overlayable.xml`
declares which resources are overlappable, including the crucial ones:

```xml
<item type="array" name="additional_cell_broadcast_receiver_packages" />
<item type="array" name="test_cell_broadcast_receiver_packages" />
...
<item type="string" name="state_local_test_alert"/>
<item type="string" name="enable_state_local_test_alerts_title"/>
```

and `packages/apps/CellBroadcastReceiver/RROSampleTestApp/` exists purely as a template for OEM
overlays (it ships only `AndroidManifest.xml` + a couple of value files).

The channel-range string arrays (`cmas_*_channels_range_strings`, `etws_*_range_strings`, …) are also
overlaid per MCC/MNC. Examples present in the AOSP tree:

Exact contents, verified by grep over the tree. There are 57 MCC-level overlay `config.xml` files
plus the global default.

| Overlay | Notable content |
| --- | --- |
| `res/values/config.xml` | global default: `allow_testing_mode_on_user_build = **true**`; `etws_test_alerts_range_strings` = `0x1103:rat=gsm, emergency=true`; `required_monthly_test_range_strings` = `0x111C` (+`0x1129`); `exercise_alert_range_strings` = `0x111D` (+`0x112A`); `operator_defined_alert_range_strings` = `0x111E` (+`0x112B`); `additional_cbs_channels_strings`, `emergency_alerts_channels_range_strings`, `public_safety_messages_channels_range_strings`, `state_local_test_alert_range_strings` all **empty** |
| `res/values-mcc440-mnc20/config.xml` | the only overlay that sets `allow_testing_mode_on_user_build = **false**`; also overrides `etws_test_alerts_range_strings` to `0x1103:rat=gsm, emergency=true, **debug_build=true**` |
| `res/values-mcc234/config.xml` | `operator_defined_alert_range_strings` = `0x111E:rat=gsm, emergency=true, **debug_build=true**` (note: `debug_build`, not `testing_mode`) |
| `res/values-mcc284/config.xml` | `exercise_alert_range_strings` and `operator_defined_alert_range_strings` marked `**testing_mode=true**`; also defines `state_local_test_alert_range_strings` = `0x112E`, `0x112F` (without `testing_mode`); `etws_*` arrays emptied |
| `res/values-mcc424/config.xml` | `state_local_test_alert_range_strings` = `0x112E`, `0x112F` with `**testing_mode=true**`; `public_safety_messages_channels_range_strings` = `0x112C`, `0x112D`; monthly test retyped `type=info`; `cmas_amber_*` and `operator_defined_*` emptied |
| `res/values-mcc450-mnc05/config.xml` (Korea) | adds `override_dnd=true, always_on=true` for presidential; adds `**testing_mode=true**` channels `0xA000`, `0xA001`, `0xA002`–`0xA009`; `additional_cbs_channels_strings` = `0xA00A-0xAFFF` with `display=false` |
| `res/values-mcc450-mnc06/config.xml` (Korea) | adds `**testing_mode=true**` channels `0xA16A`, `0xA16B`, `0xA16C`; `additional_cbs_channels_strings` split around `0xA16A`-`0xA16C` |

The two switch keys are therefore used in genuinely different ways by carriers: some use
`testing_mode=true` (exercise/operator-defined/state-local on mcc284 and mcc424, vendor channels on
mcc450), some use `debug_build=true` (mcc234, mcc440-mnc20).

### Established fact

`allow_testing_mode_on_user_build` is `true` in AOSP defaults but `false` in at least one carrier
overlay. An OEM is therefore free to disable the `*#*#2627#*#*` toggle on retail builds. Whether any
specific OEM does so is **UNKNOWN** and must be checked per device.

## 2. Compatibility matrix

Legend: **CONFIRMED** (AOSP-verified), **LIKELY**, **UNKNOWN — requires experimental verification**,
**NOT POSSIBLE**.

| OEM | Stock Android | Privileged test possible? | OEM customization | Notes |
| --- | --- | --- | --- | --- |
| Google Pixel | ships the CB apex with `CellBroadcastApp` (`certificate: networkstack`, privileged) and, historically, `CellBroadcastAppPlatform` on `system_ext` | **LIKELY** — Pixel is the closest to AOSP and the most likely to be flashable with a userdebug/eng image | Pixel ships the AOSP CBR app plus a Google overlay for WEA wording; `*#*#2627#*#*` expected to work on userdebug | **UNKNOWN** in detail. Pixel is the recommended first hardware target because a userdebug build is officially obtainable. |
| Samsung Galaxy | Samsung ships its own SystemUI/emergency-alert presentation and historically a Samsung-flavoured CBR | **UNKNOWN** | Samsung overrides alert wording, adds its own settings surface, and ships carrier-specific channel configuration | **UNKNOWN.** Samsung is not the reference platform and must not dictate the architecture. Needs a device to inspect. |
| Xiaomi / Redmi (MIUI/HyperOS) | heavily customized; often ships a Mi-flavoured emergency-alert app | **UNKNOWN** | HyperOS replaces large parts of SystemUI | **UNKNOWN.** Bootloader unlocking is a precondition for any flashing work and is region-policy-dependent. |
| Motorola | close to AOSP historically | **UNKNOWN** | light overlay | **UNKNOWN.** |
| Nothing | close to AOSP, small team, historically developer-friendly | **UNKNOWN** | light overlay | **UNKNOWN.** |

> No row above is filled with a guess. Each "UNKNOWN" corresponds to an experiment in
> `experiments.md` (Experiments 10–14).

## 3. How to determine an OEM's behaviour (method to be used in the experiments)

These are non-invasive and can be run against a stock device over ADB:

1. **Is the CB apex present and who owns the receiver?**
   `adb shell pm list packages | grep cellbroadcast`
   `adb shell dumpsys package com.android.cellbroadcastreceiver | head -80`
2. **Is the AOSP receiver enabled?**
   `adb shell cmd package query-receivers -a android.provider.action.SMS_EMERGENCY_CB_RECEIVED`
3. **What channel configuration is active?** — read the resource arrays via
   `adb shell dumpsys activity service ...` is not sufficient; the cleanest route is
   `adb shell cmd overlay list` plus, on a debuggable build, a direct read of the app's resources.
4. **Is testing mode permitted?** `adb shell getprop ro.debuggable`, then attempt the secret code and
   observe the toast (`testing_mode_enabled` / `testing_mode_disabled` strings).
5. **Which settings toggles are visible?** Inspect the app's settings screen and compare against
   `show_test_settings`, `show_state_local_test_settings`, etc.

## 4. Why the project must not anchor on one OEM

The mechanism we depend on is **AOSP's**, not any OEM's:

* the protected broadcast actions are declared in `frameworks/base` (AOSP),
* the receiving app is an AOSP mainline module,
* the test application is AOSP.

An OEM can:
* overlay channel configuration,
* overlay wording,
* disable the user-build testing-mode toggle,
* or (worst case) fork the receiver.

But an OEM cannot change the fact that the injection point is a protected broadcast with a
package target. That means the architecture should be validated on AOSP/Pixel first, and OEM
differences treated as *configuration*, not as architectural risk.

## 5. Recommended validation order

1. AOSP emulator (userdebug).
2. Google Pixel with a userdebug/eng build.
3. Google Pixel stock (to measure what is *not* possible).
4. Samsung Galaxy stock (to measure OEM divergence).
5. Additional OEMs only if a concrete need appears.
