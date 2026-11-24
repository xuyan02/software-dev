#!/bin/sh
# Install the sshd drop-in that exposes the Arche service account.
#
# Usage:
#   sudo ./install-sshd.sh <authorized_keys> [command-user]
#
# <authorized_keys> is the control-plane-owned file generated under
# ~/.foundry/arche-sshd/authorized_keys. sshd's AuthorizedKeysCommand runs as
# [command-user], which defaults to the sudo caller and must be able to read
# the file. The Arche server itself still runs as foundry-arche.

set -eu

authorized_keys=${1:-}
command_user=${2:-${SUDO_USER:-nobody}}
config=/etc/ssh/sshd_config.d/20-foundry-arche.conf

if [ -z "$authorized_keys" ]; then
    echo "usage: $0 <authorized_keys> [command-user]" >&2
    exit 64
fi
if [ "$(id -u)" -ne 0 ]; then
    echo "install-sshd.sh must run as root" >&2
    exit 77
fi
if [ ! -r "$authorized_keys" ]; then
    echo "cannot read authorized_keys: $authorized_keys" >&2
    exit 66
fi
if ! runuser -u "$command_user" -- test -r "$authorized_keys"; then
    echo "user $command_user cannot read $authorized_keys" >&2
    echo "choose a command user that can, e.g. the operator account" >&2
    exit 77
fi

cat > "$config" <<EOF
# Managed by Arche's design-space sshd installer.
#
# Every key in the control-plane file carries a per-repository forced command
# that runs /usr/local/bin/foundry-arche-stdio. Do not set ForceCommand here:
# it would override that per-key command.
Match User foundry-arche
    AuthorizedKeysCommand /usr/local/bin/foundry-arche-authorized-keys %u %k $authorized_keys
    AuthorizedKeysCommandUser $command_user
    PasswordAuthentication no
    KbdInteractiveAuthentication no
    PermitTTY no
    AllowTcpForwarding no
    AllowAgentForwarding no
    X11Forwarding no
    PermitTunnel no
Match all
EOF

chmod 0644 "$config"

sshd -t
if systemctl list-unit-files | grep -q '^ssh\.service'; then
    systemctl reload ssh
else
    systemctl reload sshd
fi

echo "Installed $config and reloaded sshd."
