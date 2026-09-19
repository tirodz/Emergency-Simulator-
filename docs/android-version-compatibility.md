# Android version compatibility

Verified by inspecting three concrete release branches of
`packages/apps/CellBroadcastReceiver`:

| Branch | Commit |
| --- | --- |
| `android14-release` | `346bb742baaac29cc9509a39e9f9419647f994e7` |
| `android15-release` | `62e355afa0062c10687f991a8ed7e0405f641003` |
| `android16-release` | `b97c8a4ffa3946d7206808bf4810746678b44a5c` |

and the `main` branch of `packages/modules/CellBroadcastService`
(`16ff738f4b4d8bfdffef4df4682230571caa67df`).

## 1. What is identical across 14 / 15 / 16

| Property | 14 | 15 | 16 |
| --- | --- | --- | --- |
| Two mainline components (CBR app + CBS module) in the CB apex | yes | yes | yes |
| Test app exists at `tests/testapp` | yes | yes | yes |
| Test app `sharedUserId=android.uid.phone` | yes | yes | yes |
| Test app `uses-permission BROADCAST_SMS`, `INTERACT_ACROSS_USERS_FULL` | yes | yes | yes |
| Test app `certificate: "platform"` | yes | yes | yes |
| `CellBroadcastApp` `certificate: "networkstack"`, `privileged: true` | yes | yes | yes |
| `CellBroadcastAppPlatform` `certificate: "platform"`, `system_ext_specific`, `privileged` | yes | yes | yes |
| `allow_testing_mode_on_user_build` in AOSP `values/config.xml` | `true` | `true` | `true` |
| Secret code `*#*#2627#*#*` gated by `ro.debuggable==1 \|\| allow_testing_mode_on_user_build` | yes | yes | yes |
| Protected broadcast actions `SMS_CB_RECEIVED`, `SMS_EMERGENCY_CB_RECEIVED` (frameworks/base) | yes | yes | yes |
| Test app CMAS broadcast uses `RECEIVE_EMERGENCY_BROADCAST` + `OP_RECEIVE_EMERGECY_SMS` | yes | yes | yes |
| Test app generic/ETWS broadcast uses `RECEIVE_SMS` + `OP_RECEIVE_SMS` | yes | yes | yes |

The `sendBroadcast` helper in `SendTestMessages.java` is byte-for-byte identical between
`android14-release` and `android16-release`.

## 2. What differs

| Property | 14 | 15 | 16 |
| --- | --- | --- | --- |
| `CellBroadcastApp` `updatable: true` in `Android.bp` | not present in 14 | not present in 15 | **present in 16** |
| `CellBroadcastDefaults` java_defaults block | absent in 14 (fields inline) | present | present |
| Anticipated message-format evolution (`SmsCbMessage` public constructor visibility, `isEtwsMessage`, `getCmasWarningInfo`) | present | present | present |

The `updatable: true` change in 16 means the CBR app is delivered through the mainline/APEX update
mechanism, which is relevant to OEM compatibility because it means the *app* can be updated
independently of the system image, while the *privilege* configuration (allowlist, partition) cannot.

## 3. Compatibility matrix

Legend: **CONFIRMED** = verified from source; **LIKELY** = strongly implied, not yet exercised;
**UNKNOWN** = not determined; **NOT POSSIBLE** = closed by source; **REQUIRES DEV BUILD** = needs
userdebug/eng or a custom image.

| | Android 14 | Android 15 | Android 16 |
| --- | --- | --- | --- |
| CellBroadcast architecture | Two mainline modules in CB apex (**CONFIRMED**) | same (**CONFIRMED**) | same (**CONFIRMED**) |
| AOSP test application present | **CONFIRMED** | **CONFIRMED** | **CONFIRMED** |
| Test mechanism reaches genuine receiver | **CONFIRMED** (source trace) | **CONFIRMED** (source trace) | **CONFIRMED** (source trace) |
| Stock retail feasibility | **NOT POSSIBLE** (platform signing + phone UID) | **NOT POSSIBLE** | **NOT POSSIBLE** |
| ADB-only feasibility | **NOT POSSIBLE** (shell UID rejected) | **NOT POSSIBLE** | **NOT POSSIBLE** |
| Rooted-device feasibility | **LIKELY** | **LIKELY** | **LIKELY** |
| Development-build feasibility (AOSP userdebug/eng, emulator, custom ROM) | **CONFIRMED by construction** | **CONFIRMED by construction** | **CONFIRMED by construction** |
| CBR testing mode available on user build (AOSP defaults) | **CONFIRMED** | **CONFIRMED** | **CONFIRMED** |
| `debug_build=true` channels available | requires `ro.debuggable==1` (**CONFIRMED**) | same | same |
| App is independently updatable | no | no | **CONFIRMED yes** |

## 4. Version-specific caveats that still need checking

* **`SmsCbMessage` constructor visibility per SDK level.** A device-side helper that must *construct*
  a message needs the hidden constructors. Whether those constructors are reachable on a given
  release is a runtime/reflection question, not a source question. -> experiment on a real device.
* **Android 16 QPR / feature flags.** The CBR repo contains a `flags/` directory, indicating
  feature-flagged behaviour. Any flag that changes the CBR entry point would need to be checked for
  each QPR. **Status: not yet inspected.**
* **GSI builds.** `android14-gsi` and similar branches exist. Whether a GSI ships the AOSP test app is
  **UNKNOWN** (a GSI generally ships the system image, and the test app is an `android_test` module
  that may or may not be included).

## 5. Recommended target

**Android 16 (`android16-release`) is the primary reference target**, because:

* it is the newest branch with the full architecture verified above,
* `updatable: true` makes the CBR app a mainline module, matching what a modern Pixel ships,
* the test app is unchanged relative to 14 in every way that matters for our mechanism.

Android 14 and 15 remain supported by the same mechanism; nothing in the test path is version-specific.
