#!/usr/bin/env bash
#
# Build AlertInjector into a dex jar suitable for `app_process`.
#
# Produces: android/alertinject/out/alertinject.jar
#
# Requires: an Android SDK (platforms/android-35, build-tools/35.0.0) and a JDK.
# Override the locations with ANDROID_HOME and JAVA_HOME if needed.
#
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SDK="${ANDROID_HOME:-/opt/android-sdk}"
PLATFORM="${ANDROID_PLATFORM:-android-35}"
BUILD_TOOLS="${BUILD_TOOLS:-35.0.0}"
JAVA="${JAVA_HOME:-/usr/lib/jvm/java-21-openjdk-amd64}"

ANDROID_JAR="$SDK/platforms/$PLATFORM/android.jar"
D8="$SDK/build-tools/$BUILD_TOOLS/d8"

for f in "$ANDROID_JAR" "$D8"; do
  if [ ! -e "$f" ]; then
    echo "error: missing $f" >&2
    echo "       install with: sdkmanager \"platforms;$PLATFORM\" \"build-tools;$BUILD_TOOLS\"" >&2
    exit 1
  fi
done

OUT="$HERE/out"
CLASSES="$OUT/classes"
mkdir -p "$CLASSES"

echo "==> compiling"
"$JAVA/bin/javac" -source 8 -target 8 -nowarn \
  -cp "$ANDROID_JAR" \
  -d "$CLASSES" \
  "$HERE/org/emergencysim/alertinject/AlertInjector.java"

echo "==> dexing"
"$D8" --min-api 30 --output "$OUT" \
  "$CLASSES/org/emergencysim/alertinject/AlertInjector.class"

# d8 emits classes.dex; app_process wants a jar on CLASSPATH.
( cd "$OUT" && jar cf alertinject.jar classes.dex )

echo "==> built $OUT/alertinject.jar"
ls -l "$OUT/alertinject.jar"