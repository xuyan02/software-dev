#!/bin/sh
set -eu

: "${BROWSER_CAPSULE_VIEWPORT:=1280x800}"
window_size="$(printf '%s' "$BROWSER_CAPSULE_VIEWPORT" | tr 'x' ',')"

exec chromium \
    --headless=new \
    --no-sandbox \
    --disable-gpu \
    --disable-dev-shm-usage \
    --no-first-run \
    --no-default-browser-check \
    --remote-debugging-address=0.0.0.0 \
    --remote-debugging-port=9222 \
    --remote-allow-origins='*' \
    --force-device-scale-factor=1 \
    --user-data-dir=/tmp/chromium \
    --window-size="$window_size" \
    about:blank
