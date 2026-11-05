#!/usr/bin/env bash
# Build a debug APK for an arm64 phone, install it over USB and restart the app.
set -euo pipefail
cd "$(dirname "$0")/../app"
. ../scripts/android-env.sh
apk=src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk
rm -f "$apk"
npx tauri android build --debug --apk --target aarch64 > /tmp/timlo-android-build.log 2>&1 || {
  grep -E 'error|Error|e: ' /tmp/timlo-android-build.log | head -20; exit 1; }
adb install -r "$apk" | tail -1
adb shell am force-stop dev.timlo.planner
adb shell monkey -p dev.timlo.planner -c android.intent.category.LAUNCHER 1 > /dev/null 2>&1
echo "installed and started"
