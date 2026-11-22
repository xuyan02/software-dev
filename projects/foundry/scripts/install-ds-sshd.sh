#!/usr/bin/env bash
# Install the host-sshd transport used by agent design-space remotes.
#
# The install is idempotent. Run it after rebuilding the binaries:
#
#   cargo build --bins
#   sudo scripts/install-ds-sshd.sh
#
# The script creates one service account, installs the forced command, writes
# an sshd drop-in, and grants the service account access to the design-space
# repositories. It does not run a second sshd.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
exec "$ROOT/projects/foundry/deploy/ds-ssh/install.sh" "$@"
