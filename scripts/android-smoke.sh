#!/usr/bin/env bash
# Install, launch and relaunch a debug APK on one connected Android device.
#
# Used by .github/workflows/android.yml after the emulator runner has booted.
# It deliberately avoids Gradle so the same script can be run against a phone
# over USB or wireless ADB once the APK has been downloaded.
set -euo pipefail

PACKAGE="${PACKAGE:-app.notebox}"
ACTIVITY="${ACTIVITY:-dev.dioxus.main.MainActivity}"
APK="${APK:-dist/android/notebox-debug-x86_64.apk}"
ARTIFACTS_DIR="${ARTIFACTS_DIR:-.}"

log() {
    printf '%s\n' "$*"
}

capture() {
    mkdir -p "$ARTIFACTS_DIR"
    adb exec-out screencap -p >"$ARTIFACTS_DIR/android-smoke.png" 2>/dev/null || true
    adb logcat -d >"$ARTIFACTS_DIR/android-logcat.txt" 2>/dev/null || true
    adb shell dumpsys activity activities \
        >"$ARTIFACTS_DIR/android-activities.txt" 2>&1 || true
    adb shell run-as "$PACKAGE" ls -l files \
        >"$ARTIFACTS_DIR/android-files.txt" 2>&1 || true
}

status=0
trap 'status=$?; if [ "$status" -ne 0 ]; then log "Smoke test failed; capturing evidence"; capture; fi; exit "$status"' EXIT

test -f "$APK" || {
    log "APK not found: $APK"
    exit 1
}

mkdir -p "$ARTIFACTS_DIR"
log "Device: $(adb shell getprop ro.product.model | tr -d '\r')"
log "Android: $(adb shell getprop ro.build.version.release | tr -d '\r') (API $(adb shell getprop ro.build.version.sdk | tr -d '\r'))"
log "Installing $APK"
adb install -r "$APK"

start_and_wait() {
    local pid=""
    local attempt=0
    log "Starting $PACKAGE/$ACTIVITY"
    adb shell am start -W -n "$PACKAGE/$ACTIVITY"

    while [ "$attempt" -lt 30 ]; do
        pid="$(adb shell pidof "$PACKAGE" | tr -d '\r' || true)"
        if [ -n "$pid" ]; then
            break
        fi
        attempt=$((attempt + 1))
        sleep 1
    done
    if [ -z "$pid" ]; then
        log "App process did not start"
        exit 1
    fi
    log "App pid: $pid"

    attempt=0
    while [ "$attempt" -lt 30 ]; do
        if adb shell dumpsys activity activities \
            | grep -E "mResumedActivity|topResumedActivity" \
            | grep -F "$PACKAGE" >/dev/null; then
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 1
    done
    log "App is not the resumed activity"
    exit 1
}

start_and_wait
sleep 10
adb exec-out screencap -p >"$ARTIFACTS_DIR/android-smoke-launch.png"
sleep 10
adb shell run-as "$PACKAGE" ls -l files >"$ARTIFACTS_DIR/android-files.txt" 2>&1 || true
adb shell dumpsys activity activities >"$ARTIFACTS_DIR/android-activities.txt" 2>&1 || true
adb exec-out screencap -p >"$ARTIFACTS_DIR/android-smoke.png"
adb logcat -d >"$ARTIFACTS_DIR/android-logcat.txt"

if grep -q "FATAL EXCEPTION" "$ARTIFACTS_DIR/android-logcat.txt"; then
    log "The app crashed during the first launch"
    grep -B2 -A40 "FATAL EXCEPTION" "$ARTIFACTS_DIR/android-logcat.txt" || true
    exit 1
fi

log "Force-stopping and relaunching"
adb shell am force-stop "$PACKAGE"
sleep 2
start_and_wait
sleep 10
adb exec-out screencap -p >"$ARTIFACTS_DIR/android-smoke-relaunch.png"
log "Smoke test passed"
