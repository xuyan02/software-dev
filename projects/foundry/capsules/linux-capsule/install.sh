#!/usr/bin/env bash
#
# One-time host setup for the linux-capsule library.
#
# Run this as your normal user (not root). It uses sudo only for package
# installation and /etc/subuid updates. It is idempotent:
#   - installs Podman and uidmap if missing;
#   - adds a non-overlapping subuid/subgid range for the current user;
#   - verifies rootless user namespaces;
#   - creates the Podman network used by capsulees;
#   - builds the desktop base image with the screenshot sidecar.
#
# It does not start a capsule; use `cargo run --bin linuxcapsule -- demo`.
set -euo pipefail
cd "$(dirname "$0")"

IMAGE="${IMAGE:-localhost/linux-capsule:1}"
NETWORK="${NETWORK:-linux-capsule}"
REBUILD=0
SKIP_PACKAGES=0
SKIP_IMAGE=0
SKIP_SMOKE=0
ENABLE_LINGER=0
TMP_DIR="$(mktemp -d)"
SMOKE_CONTAINERS=()
cleanup() {
    for container in "${SMOKE_CONTAINERS[@]}"; do
        podman rm -f "$container" >/dev/null 2>&1 || true
    done
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT

usage() {
    cat <<EOF
Usage: ./install.sh [options]

  --image NAME        Base image tag (default: $IMAGE)
  --network NAME      Podman network (default: $NETWORK)
  --rebuild           Rebuild the image even if it exists
  --skip-packages     Do not install Podman/uidmap
  --skip-image        Do not build the base image
  --skip-smoke        Do not run the container smoke test
  --linger            Enable systemd lingering for this user
  -h, --help          Show this help
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --image) IMAGE="$2"; shift 2 ;;
        --network) NETWORK="$2"; shift 2 ;;
        --rebuild) REBUILD=1; shift ;;
        --skip-packages) SKIP_PACKAGES=1; shift ;;
        --skip-image) SKIP_IMAGE=1; shift ;;
        --skip-smoke) SKIP_SMOKE=1; shift ;;
        --linger) ENABLE_LINGER=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *) echo "unknown option: $1" >&2; usage >&2; exit 2 ;;
    esac
done

if [[ "$(id -u)" -eq 0 ]]; then
    echo "Run install.sh as your normal user, not root. It will use sudo where needed." >&2
    exit 1
fi

USER_NAME="$(id -un)"
SUDO="sudo"

echo "==> linux-capsule host setup"
echo "    user:    $USER_NAME"
echo "    image:   $IMAGE"
echo "    network: $NETWORK"

if ! command -v sudo >/dev/null 2>&1; then
    echo "sudo is required." >&2
    exit 1
fi
sudo -v

# ---------------------------------------------------------------------------
# Packages
# ---------------------------------------------------------------------------
if [[ "$SKIP_PACKAGES" -eq 0 ]]; then
    if command -v apt-get >/dev/null 2>&1; then
        echo "==> installing Podman packages"
        sudo apt-get update
        sudo apt-get install -y podman uidmap slirp4netns fuse-overlayfs
        for optional in containernetworking-plugins passt netavark aardvark-dns; do
            if apt-cache show "$optional" >/dev/null 2>&1; then
                sudo apt-get install -y "$optional"
            fi
        done
    elif command -v dnf >/dev/null 2>&1; then
        echo "==> installing Podman packages"
        sudo dnf install -y podman shadow-utils
    else
        echo "Unsupported package manager. Install podman and uidmap manually." >&2
        exit 1
    fi
else
    echo "==> skipping package installation"
fi

if ! command -v podman >/dev/null 2>&1; then
    echo "podman was not found after installation." >&2
    exit 1
fi

# ---------------------------------------------------------------------------
# subuid / subgid
# ---------------------------------------------------------------------------
has_subid() {
    local file="$1"
    local user="$2"
    [[ -f "$file" ]] && awk -F: -v u="$user" '$1 == u { found = 1 } END { exit !found }' "$file"
}

next_subid_start() {
    local start
    start="$(awk -F: 'BEGIN { max = 100000 } { end = $2 + $3; if (end > max) max = end } END { print max }' \
        /etc/subuid /etc/subgid 2>/dev/null || true)"
    [[ -n "$start" ]] || start=100000
    printf '%s\n' "$start"
}

configure_subids() {
    local user="$1"
    local need_uid=1
    local need_gid=1
    if has_subid /etc/subuid "$user"; then
        need_uid=0
    fi
    if has_subid /etc/subgid "$user"; then
        need_gid=0
    fi

    if [[ "$need_uid" -eq 0 && "$need_gid" -eq 0 ]]; then
        echo "==> subuid/subgid already configured for $user"
        return
    fi

    [[ -e /etc/subuid ]] || sudo touch /etc/subuid
    [[ -e /etc/subgid ]] || sudo touch /etc/subgid

    local start end
    start="$(next_subid_start)"
    end=$((start + 65535))
    if ! [[ "$start" =~ ^[0-9]+$ ]] || (( start < 100000 )); then
        echo "refusing to allocate a suspicious subuid range: $start" >&2
        exit 1
    fi

    echo "==> configuring subuid/subgid for $user: ${start}-${end}"
    local args=()
    if [[ "$need_uid" -eq 1 ]]; then
        args+=(--add-subuids "${start}-${end}")
    fi
    if [[ "$need_gid" -eq 1 ]]; then
        args+=(--add-subgids "${start}-${end}")
    fi
    if ! sudo usermod "${args[@]}" "$user"; then
        cat >&2 <<EOF
Could not update subordinate IDs automatically. Add them manually:

  echo "$user:$start:65536" | sudo tee -a /etc/subuid
  echo "$user:$start:65536" | sudo tee -a /etc/subgid
  podman system migrate

EOF
        exit 1
    fi
}

configure_subids "$USER_NAME"

# Apply the new mappings to the rootless Podman pause process.
echo "==> applying mappings (podman system migrate)"
podman system migrate || true

# ---------------------------------------------------------------------------
# Verification
# ---------------------------------------------------------------------------
echo "==> verifying rootless Podman"
ROOTLESS="$(podman info --format '{{.Host.Security.Rootless}}' 2>/dev/null || true)"
if [[ "$ROOTLESS" != "true" ]]; then
    echo "Podman is not running rootless. Diagnostics:" >&2
    podman info >&2 || true
    exit 1
fi

if ! podman unshare cat /proc/self/uid_map >/dev/null 2>&1; then
    echo "podman unshare failed. Check /etc/subuid, /etc/subgid and newuidmap/newgidmap." >&2
    podman unshare cat /proc/self/uid_map >&2 || true
    exit 1
fi
echo "    $(podman --version)"
echo "    rootless: true"
echo "    uid_map:"
podman unshare cat /proc/self/uid_map | sed 's/^/      /'

# ---------------------------------------------------------------------------
# Network
# ---------------------------------------------------------------------------
if podman network exists "$NETWORK" >/dev/null 2>&1; then
    echo "==> network $NETWORK already exists"
else
    echo "==> creating network $NETWORK"
    podman network create "$NETWORK"
fi

# ---------------------------------------------------------------------------
# Image
# ---------------------------------------------------------------------------
build_image() {
    if [[ ! -f Cargo.lock ]]; then
        if command -v cargo >/dev/null 2>&1; then
            echo "==> generating Cargo.lock"
            cargo generate-lockfile
        else
            echo "Cargo.lock is missing and cargo is not installed." >&2
            exit 1
        fi
    fi

    echo "==> building $IMAGE"
    echo "    (the first build downloads Rust and Debian images, and compiles the sidecar)"
    local build_args=()
    if [[ -n "${RUST_IMAGE:-}" ]]; then
        build_args+=(--build-arg "RUST_IMAGE=$RUST_IMAGE")
    fi
    if [[ -n "${BASE_IMAGE:-}" ]]; then
        build_args+=(--build-arg "BASE_IMAGE=$BASE_IMAGE")
    fi
    podman build "${build_args[@]}" \
        -t "$IMAGE" \
        -f images/linux-capsule/Containerfile \
        .
}

if [[ "$SKIP_IMAGE" -eq 0 ]]; then
    if [[ "$REBUILD" -eq 1 ]] || ! podman image exists "$IMAGE" >/dev/null 2>&1; then
        build_image
    else
        echo "==> image $IMAGE already exists (use --rebuild to rebuild)"
    fi
else
    echo "==> skipping image build"
fi

# ---------------------------------------------------------------------------
# Image smoke test
# ---------------------------------------------------------------------------
if [[ "$SKIP_SMOKE" -eq 0 ]]; then
    if ! podman image exists "$IMAGE" >/dev/null 2>&1; then
        echo "==> skipping smoke test: image $IMAGE is missing"
    elif ! command -v curl >/dev/null 2>&1; then
        echo "==> skipping smoke test: curl not found"
    elif ! command -v python3 >/dev/null 2>&1; then
        echo "==> skipping smoke test: python3 not found"
    else
        PORT="$(python3 - <<'PY'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print(s.getsockname()[1])
s.close()
PY
)"
        NAME="linux-capsule-smoke-$$"
        echo "==> smoke test on 127.0.0.1:$PORT"
        CID="$(podman run -d --rm --name "$NAME" \
            --network "$NETWORK" \
            --cap-drop ALL \
            --security-opt no-new-privileges \
            --tmpfs /tmp:rw,size=64m \
            --tmpfs /run:rw,size=64m \
            -e LINUX_CAPSULE_MODE=desktop \
            -e LINUX_CAPSULE_TOKEN=smoke \
            -p "127.0.0.1:${PORT}:8099" \
            "$IMAGE" xterm -geometry 80x24)"
        SMOKE_CONTAINERS+=("$CID")
        READY=0
        for _ in $(seq 1 100); do
            if curl -fsS -H 'Authorization: Bearer smoke' \
                "http://127.0.0.1:${PORT}/health" >/dev/null 2>&1; then
                READY=1
                break
            fi
            sleep 0.1
        done
        if [[ "$READY" -eq 1 ]]; then
            echo "    sidecar health OK"
        else
            echo "    sidecar did not become healthy; logs:" >&2
            podman logs "$CID" >&2 || true
            podman rm -f "$CID" >/dev/null 2>&1 || true
            exit 1
        fi
        podman rm -f "$CID" >/dev/null 2>&1 || true

        echo "==> headless smoke test"
        NAME_HEADLESS="${NAME}-headless"
        CID_HEADLESS="$(podman run -d --rm --name "$NAME_HEADLESS" \
            --network "$NETWORK" \
            --cap-drop ALL \
            --security-opt no-new-privileges \
            --tmpfs /tmp:rw,size=64m \
            --tmpfs /run:rw,size=64m \
            -e LINUX_CAPSULE_MODE=headless \
            "$IMAGE" sleep infinity)"
        SMOKE_CONTAINERS+=("$CID_HEADLESS")
        HEADLESS_READY=0
        for _ in $(seq 1 50); do
            if podman exec "$CID_HEADLESS" sh -c 'echo capsule > /tmp/inside.txt' \
                >/dev/null 2>&1; then
                HEADLESS_READY=1
                break
            fi
            sleep 0.1
        done
        if [[ "$HEADLESS_READY" -ne 1 ]]; then
            echo "    headless exec failed; logs:" >&2
            podman logs "$CID_HEADLESS" >&2 || true
            podman rm -f "$CID_HEADLESS" >/dev/null 2>&1 || true
            exit 1
        fi
        podman cp "$CID_HEADLESS:/tmp/inside.txt" "$TMP_DIR/inside.txt"
        if ! grep -q capsule "$TMP_DIR/inside.txt"; then
            echo "    headless download returned unexpected contents" >&2
            podman rm -f "$CID_HEADLESS" >/dev/null 2>&1 || true
            exit 1
        fi
        echo "hello from host" >"$TMP_DIR/upload.txt"
        podman cp "$TMP_DIR/upload.txt" "$CID_HEADLESS:/tmp/upload.txt"
        podman exec "$CID_HEADLESS" cat /tmp/upload.txt >"$TMP_DIR/downloaded.txt"
        if ! grep -q "hello from host" "$TMP_DIR/downloaded.txt"; then
            echo "    headless upload returned unexpected contents" >&2
            podman rm -f "$CID_HEADLESS" >/dev/null 2>&1 || true
            exit 1
        fi
        echo "    headless exec + upload/download OK"
        podman rm -f "$CID_HEADLESS" >/dev/null 2>&1 || true
    fi
fi

# ---------------------------------------------------------------------------
# Optional lingering
# ---------------------------------------------------------------------------
if [[ "$ENABLE_LINGER" -eq 1 ]]; then
    echo "==> enabling systemd lingering for $USER_NAME"
    sudo loginctl enable-linger "$USER_NAME"
fi

cat <<EOF

==> done

Next steps:
  cargo run --bin linuxcapsule -- demo --mode desktop
  cargo run --bin linuxcapsule -- demo --mode headless

The demo creates a capsule, exercises commands and file upload/download, and removes
the container. Desktop mode also saves /tmp/linux-capsule-before.png and
/tmp/linux-capsule-after.png and sends a click and text. The library entry point
is linux_capsule::LinuxCapsule::create.
EOF
