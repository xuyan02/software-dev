#!/bin/sh
set -eu

mode="${LINUX_CAPSULE_MODE:-desktop}"
case "$mode" in
    desktop | headless) ;;
    *)
        echo "unknown LINUX_CAPSULE_MODE: $mode" >&2
        exit 1
        ;;
esac

if [ "$mode" = "desktop" ]; then
    : "${DISPLAY:=:99}"
    : "${LINUX_CAPSULE_SCREEN:=1280x800x24}"

    Xvfb "$DISPLAY" -screen 0 "$LINUX_CAPSULE_SCREEN" -nolisten tcp &
    xvfb_pid=$!

    socket="/tmp/.X11-unix/X${DISPLAY#:}"
    i=0
    while [ ! -S "$socket" ] && [ "$i" -lt 500 ]; do
        i=$((i + 1))
        sleep 0.01
    done
    if [ ! -S "$socket" ]; then
        echo "Xvfb did not create $socket" >&2
        kill "$xvfb_pid" 2>/dev/null || true
        exit 1
    fi

    openbox >/dev/null 2>&1 &
    linux-capsule-sidecar &
    sidecar_pid=$!
fi

if [ "$#" -eq 0 ]; then
    if [ "$mode" = "desktop" ]; then
        # No app supplied: keep the desktop capsule alive for the sidecar.
        wait "$sidecar_pid"
    else
        exec sleep infinity
    fi
fi

exec "$@"
