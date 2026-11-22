#!/usr/bin/env bash
# Install the host-sshd transport for design-space SSH remotes.
#
# This follows the Git/GitLab model: one service account, many keys, and a
# forced command. The host sshd stays the only SSH server; there is no
# design-space sshd container.
#
# Run as root:
#
#   sudo projects/foundry/deploy/ds-ssh/install.sh
#
# Environment overrides:
#
#   FOUNDRY_DIR            operator's Foundry directory (default ~/.foundry)
#   FOUNDRY_DS_SSH_USER    service account (default foundry-ds)
#   FOUNDRY_DS_SSH_HOME    service account home (default /var/lib/<user>)
#   FOUNDRY_DS_REPOS_ROOT  design-space repository root (default
#                          <FOUNDRY_DIR>/design-spaces)
#   FOUNDRY_DS_SERVER      ds-server binary (default target/debug/ds-server)
set -euo pipefail

if [ "$(id -u)" -ne 0 ]; then
    echo "run as root: sudo $0" >&2
    exit 1
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_root="$(cd "$script_dir/../.." && pwd)"
repo_root="$(cd "$project_root/../.." && pwd)"

if [ -n "${SUDO_USER:-}" ]; then
    operator_user="$SUDO_USER"
elif [ -n "${PKEXEC_UID:-}" ]; then
    operator_user="$(getent passwd "$PKEXEC_UID" | cut -d: -f1)"
else
    operator_user="$USER"
fi
operator_home="$(getent passwd "$operator_user" | cut -d: -f6)"
if [ -z "$operator_home" ] || [ ! -d "$operator_home" ]; then
    echo "cannot find home directory for $operator_user" >&2
    exit 1
fi

foundry_dir="${FOUNDRY_DIR:-$operator_home/.foundry}"
service_user="${FOUNDRY_DS_SSH_USER:-foundry-ds}"
service_home="${FOUNDRY_DS_SSH_HOME:-/var/lib/$service_user}"
repos_root="${FOUNDRY_DS_REPOS_ROOT:-$foundry_dir/design-spaces}"
ds_server="${FOUNDRY_DS_SERVER:-$repo_root/target/debug/ds-server}"
ssh_root="$foundry_dir/ds-sshd"
authorized_keys="$ssh_root/authorized_keys"
sshd_dropin="/etc/ssh/sshd_config.d/10-foundry-ds.conf"

for path in "$foundry_dir" "$repos_root" "$ds_server" "$authorized_keys"; do
    case "$path" in
        *" "*)
            echo "paths used by sshd must not contain spaces: $path" >&2
            exit 1
            ;;
    esac
done

if [ ! -x "$ds_server" ]; then
    echo "ds-server is not executable: $ds_server" >&2
    echo "build it with: cd $repo_root && cargo build --bins" >&2
    exit 1
fi

echo "==> checking packages"
packages=()
if [ ! -x /usr/sbin/sshd ]; then
    packages+=(openssh-server)
fi
if ! command -v setfacl >/dev/null 2>&1; then
    packages+=(acl)
fi
if [ "${#packages[@]}" -gt 0 ]; then
    export DEBIAN_FRONTEND=noninteractive
    apt-get update
    apt-get install -y --no-install-recommends "${packages[@]}"
fi

echo "==> creating service account $service_user"
if ! id -u "$service_user" >/dev/null 2>&1; then
    useradd --system --create-home --home-dir "$service_home" \
        --shell /bin/sh "$service_user"
fi
# The forced command needs a shell, but the account must not accept a password.
# `!` in shadow locks the account for sshd even with public keys; `*` has the
# same effect for password authentication without blocking key authentication.
usermod -p '*' "$service_user"
install -d -m 0750 -o "$service_user" -g "$service_user" "$service_home"
install -d -m 0700 -o "$service_user" -g "$service_user" "$service_home/.ssh"
install -m 0600 -o "$service_user" -g "$service_user" /dev/null \
    "$service_home/.ssh/authorized_keys"

echo "==> installing forced-command helpers"
install -m 0755 "$script_dir/foundry-ds-stdio" /usr/local/bin/foundry-ds-stdio
install -m 0755 "$script_dir/foundry-ds-authorized-keys" \
    /usr/local/bin/foundry-ds-authorized-keys

echo "==> preparing ssh metadata and repository permissions"
install -d -m 0700 -o "$operator_user" -g "$operator_user" "$foundry_dir"
install -d -m 0755 -o "$operator_user" -g "$operator_user" \
    "$ssh_root" "$ssh_root/allowlists"
touch "$authorized_keys"
chown "$operator_user:$operator_user" "$authorized_keys"
chmod 0644 "$authorized_keys"
if [ -d "$ssh_root/allowlists" ]; then
    find "$ssh_root/allowlists" -type f -exec chmod 0644 {} +
fi

# The service account must traverse the operator's home and Foundry directory,
# and mutate the design-space repositories.
setfacl -m "u:$service_user:x" "$operator_home" "$foundry_dir"
if [ -d "$repos_root" ]; then
    setfacl -R -m "u:$service_user:rwX,u:$operator_user:rwX" "$repos_root"
    setfacl -R -d -m "u:$service_user:rwX,u:$operator_user:rwX" "$repos_root"
    find "$repos_root" -name refs.lock -type f -exec chmod 0664 {} +
fi

if command -v runuser >/dev/null 2>&1; then
    if ! runuser -u "$service_user" -- test -r "$authorized_keys"; then
        echo "warning: $service_user cannot read $authorized_keys" >&2
    fi
    if ! runuser -u "$service_user" -- test -x "$ds_server"; then
        echo "warning: $service_user cannot execute $ds_server" >&2
        echo "warning: set FOUNDRY_DS_SERVER to a path the service account can reach" >&2
    fi
fi

echo "==> writing sshd configuration"
install -d -m 0755 /etc/ssh/sshd_config.d
backup="$(mktemp)"
cp -a /etc/ssh/sshd_config "$backup"
if ! grep -Eq '^[[:space:]]*Include[[:space:]]+/etc/ssh/sshd_config\.d/\*\.conf' \
    /etc/ssh/sshd_config; then
    temporary="$(mktemp)"
    {
        printf '%s\n' 'Include /etc/ssh/sshd_config.d/*.conf'
        cat /etc/ssh/sshd_config
    } > "$temporary"
    install -m 0644 "$temporary" /etc/ssh/sshd_config
    rm -f "$temporary"
fi

cat > "$sshd_dropin" <<EOF
# Managed by Foundry's design-space sshd installer.
#
# All agent keys are served from a control-plane-owned file. The command runs
# as the service account and has no effect for other SSH users.
AuthorizedKeysCommand /usr/local/bin/foundry-ds-authorized-keys %u $service_user $authorized_keys
AuthorizedKeysCommandUser $service_user

Match User $service_user
    PasswordAuthentication no
    KbdInteractiveAuthentication no
    PermitTTY no
    AllowTcpForwarding no
    AllowAgentForwarding no
    X11Forwarding no
    PermitTunnel no
Match all
EOF
chmod 0644 "$sshd_dropin"

echo "==> validating sshd configuration"
install -d -m 0755 -o root -g root /run/sshd
if ! /usr/sbin/sshd -t; then
    echo "sshd rejected the generated configuration; rolling back" >&2
    rm -f "$sshd_dropin"
    cp -a "$backup" /etc/ssh/sshd_config
    rm -f "$backup"
    exit 1
fi
rm -f "$backup"

echo "==> restarting sshd"
if systemctl is-active --quiet ssh.socket; then
    systemctl restart ssh.socket
elif systemctl cat ssh.service >/dev/null 2>&1; then
    systemctl enable --now ssh >/dev/null 2>&1 || true
    systemctl restart ssh
elif systemctl cat sshd.service >/dev/null 2>&1; then
    systemctl enable --now sshd >/dev/null 2>&1 || true
    systemctl restart sshd
else
    service ssh restart || service sshd restart
fi

echo "design-space SSH is served by the host sshd"
echo "  service account: $service_user"
echo "  repos:           $repos_root"
echo "  authorized_keys: $authorized_keys"
echo "  sshd drop-in:    $sshd_dropin"
echo
echo "Set in $foundry_dir/foundry.toml and restart the control plane:"
echo "  design_space_ssh_url = \"ssh://$service_user@<host>\""
