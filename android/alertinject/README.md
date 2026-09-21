# alertinject — development-only Cell Broadcast test injector

A single Java class that constructs an ETWS **test** `SmsCbMessage` and delivers it to the stock
`CellBroadcastReceiver` on a device the developer controls, so that Android itself runs its real
emergency-alert processing.

This is the proof of concept for the Emergency Simulator project. It is not a product. The
controller that will eventually drive it does not exist yet.

## What it does

```
AlertInjector (root, via app_process)
  → builds SmsCbMessage reflectively
  → broadcasts android.provider.action.SMS_EMERGENCY_CB_RECEIVED
  → stock CellBroadcastReceiver
  → stock CellBroadcastAlertService
  → real alert UI + sound + text-to-speech
```

Verified working on Android 15 / API 35 `userdebug`. See `docs/experiments.md`, EXP-ALERT-002.

## What it does not do

* It does not transmit anything. There is no modem, radio or network involvement. The message is
  injected locally into the receiver on this device and cannot reach any other device or any
  cellular network.
* It cannot send a real hazard category. The warning type is pinned to
  `ETWS_WARNING_TYPE_TEST_MESSAGE` (0x03) and there is no argument or option to change it.
* It will not send a body that does not begin with `TEST`.

## Requirements

* A device you own and control, with root. On a `userdebug` or `eng` AOSP build,
  `adb root` provides this.
* The receiving app must accept test alerts. Run the vendor secret code first:
  `adb shell am broadcast -a android.telephony.action.SECRET_CODE -d "android_secret_code://2627"`.
  If the "test alerts" toggle is hidden on the build, `enable_test_alerts=true` must also be set in
  `/data/user_de/0/<cb-package>/shared_prefs/<cb-package>_preferences.xml` and the app force-stopped.
  Without these, the message is silently dropped by preference filtering *after* passing every
  permission check.

## Building

```bash
./build.sh
```

Needs an Android SDK with `platforms/android-35` and `build-tools/35.0.0`, plus a JDK. Override
locations with `ANDROID_HOME` and `JAVA_HOME`. Produces `out/alertinject.jar`.

## Running

```bash
./run.sh                              # ETWS test channel, default body
./run.sh 4355 com.google.android.cellbroadcastreceiver "TEST ALERT - SIMULATION"
```

Or by hand:

```bash
adb root
adb push out/alertinject.jar /data/local/tmp/
CB_PACKAGE="$(adb shell pm list packages | sed -n 's/^package://p' | grep -i cellbroadcast | grep -i receiver | head -n 1 | tr -d '\r')"
adb shell "CLASSPATH=/data/local/tmp/alertinject.jar app_process /system/bin \
    org.emergencysim.alertinject.AlertInjector 4355 '$CB_PACKAGE' 'TEST ALERT - SIMULATION'"
```

Watch what the system does with it:

```bash
adb logcat | grep -E 'CellBroadcastReceiver:|CBAlertService|CellBroadcastAlertAudio|CellBroadcastAlertDialog'
```

## Channel choice

`4355` (`0x1103`) is the ETWS test channel, listed in AOSP's
`res/values/config.xml` as `etws_test_alerts_range_strings`. It is the only channel this tool is
meant to be used with. Other values will be rejected by the receiver's channel classification and
logged as `received undefined channels`.

## Why reflection

`SmsCbMessage`, `SmsCbEtwsInfo` and `SmsCbLocation` are `@hide`. They cannot be linked against at
compile time with a public SDK, but they are present in the boot classpath at runtime. The tool
therefore builds them reflectively. The constructor arities are taken from the AOSP source and are
recorded in the class javadoc.