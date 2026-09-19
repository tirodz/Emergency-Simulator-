# Source registry

Every factual claim in this repository should be traceable to one of the sources below. Each entry
records the repository, the branch, and the exact commit SHA that was inspected, so that a future
engineer can reproduce the reading.

Fetch pattern used (gitiles):

```
https://android.googlesource.com/platform/<repo>/+/refs/heads/<branch>/<path>?format=TEXT   # base64
```

## Primary AOSP sources inspected

### packages/apps/CellBroadcastReceiver

Local clone: `git clone --depth 1 https://android.googlesource.com/platform/packages/apps/CellBroadcastReceiver`

| Branch | Commit |
| --- | --- |
| `main` | `17f1a4acf0db7133581f006498a511266e58ee57` (2025-03-10) |
| `android16-release` | `b97c8a4ffa3946d7206808bf4810746678b44a5c` |
| `android15-release` | `62e355afa0062c10687f991a8ed7e0405f641003` |
| `android14-release` | `346bb742baaac29cc9509a39e9f9419647f994e7` |

Files that matter:

| File | Why it matters |
| --- | --- |
| `AndroidManifest.xml` | Receiver/service/activity declarations, requested permissions |
| `Android.bp` | `privileged: true`, `certificate: "networkstack"` / `"platform"` |
| `apex/permissions/*.xml` | The privapp-permissions allowlist actually granted at runtime |
| `src/com/android/cellbroadcastreceiver/CellBroadcastReceiver.java` | Entry point, secret-code testing mode |
| `src/com/android/cellbroadcastreceiver/CellBroadcastAlertService.java` | Alert classification, filtering, dispatch to audio + dialog |
| `src/com/android/cellbroadcastreceiver/CellBroadcastAlertAudio.java` | Sound and vibration |
| `src/com/android/cellbroadcastreceiver/CellBroadcastAlertDialog.java` | Full-screen UI, wake, dismissal |
| `src/com/android/cellbroadcastreceiver/CellBroadcastChannelManager.java` | Channel-range parsing, test-mode/debug-build gating |
| `src/com/android/cellbroadcastreceiver/CellBroadcastSettings.java` | Toggle visibility and enablement |
| `src/com/android/cellbroadcastreceiver/CellBroadcastConfigService.java` | `CbConfig` pushed down to the modem interface |
| `res/values/config.xml` | Channel ranges, defaults, `allow_testing_mode_on_user_build` |
| `tests/testapp/**` | The AOSP Cell Broadcast test application |

### packages/modules/CellBroadcastService

Local clone: `git clone --depth 1 https://android.googlesource.com/platform/packages/modules/CellBroadcastService`
Commit inspected: `main` = `16ff738f4b4d8bfdffef4df4682230571caa67df`

| File | Why it matters |
| --- | --- |
| `AndroidManifest.xml` | Service + provider declarations, requested permissions; confirmed `sharedUserId="android.uid.networkstack"` and `android:process="com.android.networkstack.process"` |
| `src/com/android/cellbroadcastservice/CellBroadcastHandler.java` | **Where the real `ACTION_SMS_EMERGENCY_CB_RECEIVED` broadcast is created and sent** (lines ~777–822: `FLAG_RECEIVER_FOREGROUND`, explicit package targets from `getDefaultCBRPackageName()` + `additional_cell_broadcast_receiver_packages`, and a `ro.debuggable`-gated extra broadcast to `test_cell_broadcast_receiver_packages`) |
| `src/com/android/cellbroadcastservice/GsmCellBroadcastHandler.java` | GSM/UMTS PDU → `SmsCbMessage` decoding |
| `src/com/android/cellbroadcastservice/SmsCbConstants.java` | Message identifiers (ETWS/CMAS/PWS ranges) |
| `src/com/android/cellbroadcastservice/CellBroadcastProvider.java` | History database |

### frameworks/base

| Path | Why it matters |
| --- | --- |
| `core/res/AndroidManifest.xml` | Permission protection levels; `<protected-broadcast>` declarations |
| `telephony/java/android/telephony/SmsCbMessage.java` | The message object; `@hide`/`@SystemApi` visibility |
| `telephony/java/android/telephony/SmsCbCmasInfo.java`, `SmsCbEtwsInfo.java` | CMAS/ETWS classification |
| `telephony/java/android/telephony/ICellBroadcastService.aidl` | The modem→AP injection AIDL |
| `core/java/android/provider/Telephony.java` | `Sms.Intents.SMS_CB_RECEIVED_ACTION`, `ACTION_SMS_EMERGENCY_CB_RECEIVED` |
| `core/java/android/app/AppOpsManager.java` | `OP_RECEIVE_SMS`, `OP_RECEIVE_EMERGECY_SMS` |
| `services/core/java/com/android/server/am/BroadcastController.java` | Protected-broadcast enforcement (the hard security gate) |
| `services/core/java/com/android/server/am/BroadcastSkipPolicy.java` | **Receiver-side permission + AppOp enforcement** (`requiredPermissions`, `noteOpForManifestReceiver`, registered-receiver `filter.requiredPermission`) |
| `services/core/java/com/android/server/am/BroadcastQueueModernImpl.java` | Delivery loop (inspected) |
| `services/core/java/com/android/server/am/ActivityManagerService.java` | `checkComponentPermission` |
| `core/java/android/app/LoadedApk.java` | (inspected; the relevant permission enforcement is not here) |

> Note on `Telephony.java`: it lives at `core/java/android/provider/Telephony.java`, **not** under
> `telephony/`. The first fetch attempt under `telephony/java/android/provider/` returned
> `NOT_FOUND`; the correct path is recorded here so a future session does not repeat the mistake.

### frameworks/opt/telephony

| Path | Why it matters |
| --- | --- |
| `src/java/com/android/internal/telephony/CellBroadcastServiceManager.java` | Binds to the CB service and forwards raw modem messages via `ICellBroadcastService` |

## Binary artefacts inspected

These were read directly, not quoted from a web page. Tooling for reading them is in `tools/`.

| Artefact | Source | Notes |
| --- | --- | --- |
| AOSP GSI system image | `https://dl.google.com/developers/android/cinnamonbun/images/gsi/aosp_x86_64-exp-CP41.260828.004.A8-16319058-9aa638ec.zip` | Android 17 / SDK 37, `user` build, `ro.debuggable=0`, `release-keys`. SHA-256 `9aa638ec20577ac4d15610527d2da2e7e3fc8388ae7ca23c2de3cb4e3df535c1`. |
| `com.android.cellbroadcast.capex` | inside the image at `/system/apex/` | Manifests as `com.android.cellbroadcast`. |
| `CellBroadcastApp.apk` | inside the APEX payload, `priv-app/CellBroadcastApp@<build>/` | The production receiver app. |
| `com.android.cellbroadcastreceiver.module.xml` | inside the APEX payload, `etc/permissions/` | The privileged-permission allowlist. |
| `framework-res.apk` | image, `/system/framework/` | Its binary `AndroidManifest.xml` carries the `<protected-broadcast>` declarations. |

The exact commands used to read these are in `docs/experiments.md`, Experiment 3.

## Official documentation

* Android Developers — Cell Broadcast / Wireless Emergency Alerts (overview and API surface).
* Android Developers — `android.telephony.SmsCbMessage` reference.
* source.android.com — Modular System Components / mainline module documentation.
* 3GPP TS 23.041 — Technical realization of Cell Broadcast Service (the protocol itself). Referenced
  only for conceptual structure; no protocol values in this repository were invented.

## Not yet verified

Anything not listed here is either marked `UNKNOWN` in the relevant document or backed only by
inspection of the sources above. Third-party articles are not used as the basis of any claim in
`report.md`.

