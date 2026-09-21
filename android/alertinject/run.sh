#!/usr/bin/env bash
#
# Inject one ETWS test alert into the genuine Android Cell Broadcast pipeline on a
# developer-owned test device.
#
# This is the manual form of the proof of concept. It is deliberately explicit: it takes a
# service category and a body on the command line and does nothing on its own.
#
# Prerequisites:
#   * adb in PATH, device authorised and connected
#   * root: the broadcast is protected, so the sending identity must be uid 0
#   * the device's test-alert preferences must be enabled (see docs/experiments.md, EXP-ALERT-003)
#   * android/alertinject/build.sh has been run
#
# SAFETY: no cellular transmission occurs. The message is injected locally into the receiver on
# this device only. It never reaches the modem or any network.
#
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
JAR="$HERE/out/alertinject.jar"
REMOTE=/data/local/tmp/alertinject.jar

# ETWS test channel 0x1103 == 4355. It is the only channel alertinject is meant to be used with.
CATEGORY="${1:-4355}"
TARGET_PACKAGE="${2:-}"
BODY="${3:-TEST ALERT - SIMULATION}"

if [ ! -f "$JAR" ]; then
  echo "error: $JAR not found; run build.sh first" >&2
  exit 1
fi

if [ -z "$TARGET_PACKAGE" ]; then
  TARGET_PACKAGE="$(adb shell pm list packages | sed -n 's/^package://p' | grep -i cellbroadcast | grep -i receiver | head -n 1 | tr -d '\r')"
fi

if [ -z "$TARGET_PACKAGE" ]; then
  echo "error: no CellBroadcast receiver package was detected" >&2
  exit 1
fi

case "$BODY" in
  TEST*) ;;
  *) echo "error: the body must begin with TEST (refusing to send)" >&2; exit 1 ;;
esac

echo "==> adb root"
adb root >/dev/null
adb wait-for-device

echo "==> pushing $JAR"
adb push "$JAR" "$REMOTE" >/dev/null

echo "==> injecting category=$CATEGORY"
echo "    target: $TARGET_PACKAGE"
echo "    body: $BODY"
echo
adb shell "CLASSPATH=$REMOTE app_process /system/bin \
    org.emergencysim.alertinject.AlertInjector '$CATEGORY' '$TARGET_PACKAGE' '$BODY'"