# Environment setup — reproducible commands

These are the exact commands used to build the Android test environment described in
[`environment.md`](environment.md). They are recorded so the setup can be repeated on any Debian 13
container, including one that (unlike this host) exposes `/dev/kvm`.

All commands were run as the `openhands` user. `sudo` was used only for package installation and for
creating `/opt/android-sdk`. No `se`/`setenforce` changes, no kernel changes, no unsafe workarounds.

---

## 1. System packages

Debian 13 (trixie) ships **OpenJDK 21**, not 17. Installing the wrong major version fails with
`Unable to locate package`.

```bash
sudo apt-get update
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y unzip openjdk-21-jdk-headless
```

Verify:

```bash
java -version     # -> openjdk version "21.0.12.1"
javac -version    # -> javac 21.0.12.1
unzip -v | head -1
```

Notes:

* `openjdk-17-jdk-headless` does **not** exist on trixie. If a specific JDK is required by a future
  AOSP build, it must come from a different source (e.g. Adoptium tarball), not apt.
* `unzip` is not present by default. `7z` and `simg2img` are also absent.

## 2. Android SDK

The SDK is installed under `/opt/android-sdk` (system-wide, writable by the agent user):

```bash
sudo mkdir -p /opt/android-sdk
sudo chown -R openhands:openhands /opt/android-sdk

curl -sL -o /tmp/cmdtools.zip \
  https://dl.google.com/android/repository/commandlinetools-linux-11076708_latest.zip

mkdir -p /tmp/cmdtools
unzip -q /tmp/cmdtools.zip -d /tmp/cmdtools
mkdir -p /opt/android-sdk/cmdline-tools/latest
cp -r /tmp/cmdtools/cmdline-tools/* /opt/android-sdk/cmdline-tools/latest/
```

Accept licences and install packages:

```bash
export ANDROID_HOME=/opt/android-sdk
export PATH="$PATH:/opt/android-sdk/cmdline-tools/latest/bin"

yes | sdkmanager --sdk_root=/opt/android-sdk --licenses

sdkmanager --sdk_root=/opt/android-sdk \
  "platform-tools" \
  "emulator" \
  "platforms;android-35" \
  "system-images;android-35;google_apis;x86_64"
```

## 3. Environment variables

Add to the shell (or a profile) for every emulator/adb session:

```bash
export ANDROID_HOME=/opt/android-sdk
export PATH="$PATH:/opt/android-sdk/platform-tools:/opt/android-sdk/emulator:/opt/android-sdk/cmdline-tools/latest/bin"
```

## 4. Create and boot the AVD

```bash
echo "no" | avdmanager create avd \
  -n test35 \
  -k "system-images;android-35;google_apis;x86_64" \
  -d pixel_5 --force
```

### On a host WITH KVM (normal case)

```bash
emulator -avd test35 -no-window -no-audio -no-boot-anim -gpu swiftshader_indirect
```

### On a host WITHOUT KVM (this environment)

The `-accel off` flag forces software emulation (QEMU TCG). It works, but boot takes ~9 minutes.

```bash
nohup emulator -avd test35 \
  -no-window -no-audio -no-boot-anim \
  -gpu swiftshader_indirect \
  -accel off -no-snapshot -memory 2048 \
  > /tmp/emu.log 2>&1 &
```

## 5. Wait for boot

```bash
for i in $(seq 1 40); do
  s=$(adb shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')
  echo "t=$((i*20))s boot_completed='$s'"
  [ "$s" = "1" ] && { echo BOOTED; break; }
  sleep 20
done
```

Budget **at least 10 minutes** without KVM. Expect `emulator-5554  offline` for the first ~60 s, then
`device`, then several more minutes before `sys.boot_completed=1`.

## 6. Root access

```bash
adb root          # -> "restarting adbd as root"
sleep 5
adb wait-for-device
adb shell id      # -> uid=0(root) ... context=u:r:su:s0
```

`adb root` is granted because the emulator image is `userdebug` (`ro.debuggable=1`). It is **not**
available on a `user` build.

## 7. Recommendations for future sessions

* Prefer **snapshot save/load** (`emulator -avd test35 -no-snapshot-save` off, or `adb emu avd
  snapshot save`) to avoid paying 9 minutes per cold boot.
* Do not run more than one emulator: 4 vCPU total, and TCG will saturate them.
* `adb emu kill` shuts the emulator down cleanly.
* If the host gains KVM, drop `-accel off` and boot time drops to well under a minute.

## 8. Injecting a test alert — proven recipe

This is the sequence that produced the genuine Android emergency alert. Full evidence in
[`experiments.md`](experiments.md).

```bash
# 0. build the injector (once)
cd android/alertinject && ./build.sh

# 1. root and deploy
adb root && adb wait-for-device
adb push android/alertinject/out/alertinject.jar /data/local/tmp/

# 2. enable testing mode (the vendor's own switch; it is a TOGGLE)
adb shell am broadcast -a android.telephony.action.SECRET_CODE -d "android_secret_code://2627"

# 3. ensure enable_test_alerts=true in the receiver's private prefs, then restart it
#    (only needed where the settings UI hides the test-alerts toggle)
adb shell am force-stop com.google.android.cellbroadcastreceiver

# 4. inject the ETWS test alert
adb shell "CLASSPATH=/data/local/tmp/alertinject.jar app_process /system/bin \
    org.emergencysim.alertinject.AlertInjector 4355 'TEST ALERT - SIMULATION'"

# 5. observe
adb logcat | grep -E 'CellBroadcastReceiver:|CBAlertService|CellBroadcastAlertAudio|CellBroadcastAlertDialog'
```

Step 2 must be verified, not assumed: sending it twice returns testing mode to its previous state.
Step 3 exists because `enable_test_alerts` defaults to false and is a separate toggle from
`testing_mode`; without it the message is dropped with `ignoring alert of type 4355 by user
preference` *after* passing every permission check.

Per-target package names differ; on the Google module the receiver package is
`com.google.android.cellbroadcastreceiver`.

## 9. What was deliberately NOT done

* No AOSP `repo init`/`repo sync`: `repo` is not installed, and the disk/RAM budget makes a build
  infeasible (see `environment.md` §5). Documented as BLOCKED rather than attempted.
* No `mknod` for `/dev/kvm` retried: it fails with `Operation not permitted` under a device
  allowlist even as root, so further attempts are pointless.
* No SELinux mode changes.
* No changes to the host kernel or container runtime.
* No cellular transmission and no modem configuration. The injector never touches `CbConfig` or the
  radio; this was the point of the whole exercise.