#!/usr/bin/env bash
# Serve a local bare clone of this repository for agent source fetches.
#
# The daemon is read-only (`git daemon` without receive-pack). Agents fetch
# `git://<host-ip>:<port>/<repo>.git`; Arche design changes still go through
# the ds remote, not this server.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
GIT_ROOT="${FOUNDRY_GIT_ROOT:-$HOME/git}"
REPO_NAME="${FOUNDRY_GIT_REPO:-$(basename "$ROOT").git}"
PORT="${FOUNDRY_GIT_PORT:-9418}"
IMAGE="${FOUNDRY_GIT_IMAGE:-localhost/openergasterion-shell:1}"
NAME="${FOUNDRY_GIT_NAME:-foundry-git}"

mkdir -p "$GIT_ROOT"
if [ ! -d "$GIT_ROOT/$REPO_NAME" ]; then
    echo "==> creating bare repository $GIT_ROOT/$REPO_NAME"
    git init --bare -q "$GIT_ROOT/$REPO_NAME"
    git -C "$GIT_ROOT/$REPO_NAME" symbolic-ref HEAD refs/heads/master
    git -C "$ROOT" push -q "$GIT_ROOT/$REPO_NAME" --all
fi

podman rm -f "$NAME" >/dev/null 2>&1 || true
podman run -d --name "$NAME" --restart unless-stopped \
    -p "$PORT:$PORT" \
    -v "$GIT_ROOT:/git:ro" \
    "$IMAGE" \
    git daemon --reuseaddr --base-path=/git --export-all --port="$PORT" --listen=0.0.0.0

echo "local git daemon: git://<host-ip>:$PORT/$REPO_NAME"
echo "bare repository:  $GIT_ROOT/$REPO_NAME"
echo "update after new commits: git -C $ROOT push $GIT_ROOT/$REPO_NAME --all"
