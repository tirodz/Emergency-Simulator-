#!/usr/bin/env bash
#
# Run the native Cell Broadcast receiver injection on a production/unrooted device.
# This does NOT require adb root. It constructs a real SmsCbMessage locally and feeds the stock
# CellBroadcastReceiver through SMS_CB_RECEIVED; no modem, SIM network transmission, or other
# device is involved.
#
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
JAR="$HERE/out/alertinject.jar"
REMOTE=/data/local/tmp/alertinject.jar

CATEGORY="${1:-4355}"
TARGET_PACKAGE="${2:-com.google.android.cellbroadcastreceiver}"
BODY="${3:-TEST ALERT - SIMULATION}"

if [ ! -f "$JAR" ]; then
  echo "error: $JAR not found; run build.sh first" >&2
  exit 1
fi

case "$BODY" in
  TEST*) ;;
  *) echo "error: body must begin with TEST" >&2; exit 1 ;;
esac

echo "==> checking adb device"
adb get-state >/dev/null
echo "==> pushing injector"
adb push "$JAR" "$REMOTE" >/dev/null

echo "==> injecting local ETWS test through native CellBroadcastReceiver"
echo "    category: $CATEGORY"
echo "    target:   $TARGET_PACKAGE"
echo "    body:     $BODY"
echo
adb shell "CLASSPATH=$REMOTE app_process /system/bin     org.emergencysim.alertinject.AlertInjector '$CATEGORY' '$TARGET_PACKAGE' '$BODY'"
