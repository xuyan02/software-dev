#!/bin/sh
set -eu

: "${ANDROID_AVD:=capsule}"

exec emulator -avd "$ANDROID_AVD" \
    -no-window \
    -no-audio \
    -no-boot-anim \
    -no-snapshot \
    -gpu swiftshader_indirect \
    -accel on \
    -port 5554 \
    -memory 2048 \
    -camera-back none \
    -camera-front none \
    -no-metrics
