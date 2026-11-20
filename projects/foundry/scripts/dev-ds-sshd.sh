#!/usr/bin/env bash
# Build and run the local design-space sshd used by agent keys.
#
# The control plane writes ~/.foundry/ssh/authorized_keys and per-agent
# allowlists. This container exposes the SSH transport and runs
# ds-server --stdio through foundry-ds-stdio.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
FOUNDRY_DIR="${FOUNDRY_DIR:-$HOME/.foundry}"
SSH_PORT="${FOUNDRY_DS_SSH_PORT:-2222}"
DS_SERVER="${FOUNDRY_DS_SERVER_BIN:-$ROOT/target/debug/ds-server}"
IMAGE="${FOUNDRY_DS_SSHD_IMAGE:-localhost/foundry-ds-sshd:1}"
NAME="${FOUNDRY_DS_SSHD_NAME:-foundry-ds-sshd}"

if [ ! -x "$DS_SERVER" ]; then
    echo "ds-server binary not found: $DS_SERVER (run cargo build --bins)" >&2
    exit 1
fi

build="$(mktemp -d)"
trap 'rm -rf "$build"' EXIT
cp "$DS_SERVER" "$build/ds-server"
cp "$ROOT/projects/foundry/deploy/ds-sshd/Containerfile" "$build/"
cp "$ROOT/projects/foundry/deploy/ds-sshd/sshd_config" "$build/"
cp "$ROOT/projects/foundry/deploy/ds-sshd/foundry-ds-stdio" "$build/"

podman build -t "$IMAGE" "$build"
podman rm -f "$NAME" >/dev/null 2>&1 || true
podman run -d --name "$NAME" --restart unless-stopped \
    -p "${SSH_PORT}:2222" \
    -v "$FOUNDRY_DIR/design-spaces:/repos:rw" \
    -v "$FOUNDRY_DIR/ds-sshd:/run/foundry/ssh:ro" \
    "$IMAGE"

echo "foundry ds sshd listening on :$SSH_PORT"
echo "authorized_keys: $FOUNDRY_DIR/ds-sshd/authorized_keys"
