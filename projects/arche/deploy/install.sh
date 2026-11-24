#!/bin/sh
# Install Arche SSH service files.
#
# Usage:
#   sudo ./install.sh /path/to/arche [repos-root]
#
# The script does not modify sshd_config. Review and install
# sshd_config.example yourself.

set -eu

arche_bin=${1:-}
repos_root=${2:-/srv/arche}
service_user=foundry-arche

if [ -z "$arche_bin" ] || [ ! -x "$arche_bin" ]; then
    echo "usage: $0 /path/to/arche [repos-root]" >&2
    exit 64
fi
if [ "$(id -u)" -ne 0 ]; then
    echo "install.sh must run as root" >&2
    exit 77
fi

if ! id "$service_user" >/dev/null 2>&1; then
    useradd --system --home-dir "$repos_root" --shell /bin/sh "$service_user"
fi
# sshd runs the per-key forced command through the account's shell.
usermod --shell /bin/sh "$service_user"

install -d -m 0755 /usr/local/bin
install -m 0755 "$(dirname "$0")/foundry-arche-stdio" /usr/local/bin/foundry-arche-stdio
install -m 0755 "$(dirname "$0")/foundry-arche-authorized-keys" /usr/local/bin/foundry-arche-authorized-keys
install -m 0755 "$arche_bin" /usr/local/bin/arche

# Keep the operator as owner when the repository root already exists under
# their home; the service account gets access through ACLs below.
install -d -m 2775 "$repos_root"
install -d -m 0755 /etc/arche
if [ ! -f /etc/arche/keys ]; then
    install -m 0644 /dev/null /etc/arche/keys
fi

if command -v setfacl >/dev/null 2>&1; then
    # The service account must be able to traverse the parent directories of
    # a repository root that lives under an operator's home directory.
    parent=$(dirname "$repos_root")
    while [ -n "$parent" ] && [ "$parent" != "/" ]; do
        setfacl -m "u:$service_user:--x" "$parent" 2>/dev/null || true
        parent=$(dirname "$parent")
    done
    setfacl -m "u:$service_user:rwx" "$repos_root"
    find "$repos_root" -type d -exec setfacl -m "u:$service_user:rwx" {} +
    find "$repos_root" -type f -exec setfacl -m "u:$service_user:rw" {} +
    # New repositories and object files are written by the control plane, so
    # carry the service account ACL through the directory defaults.
    find "$repos_root" -type d -exec setfacl -d -m "u:$service_user:rwX" {} +
fi

cat <<EOF
Installed Arche SSH service.

1. Add public keys and repositories to /etc/arche/keys:
     ssh-ed25519 AAAA... project

2. Review deploy/sshd_config.example, install it under
   /etc/ssh/sshd_config.d/, and reload sshd.

3. Repositories live under $repos_root.
EOF
