#!/usr/bin/env bash
#
# One-time host setup for the android-capsule library.
#
# Run as your normal user (not root). It uses sudo only for package
# installation and /etc/subuid updates.
set -euo pipefail
cd "$(dirname "$0")"

IMAGE="${IMAGE:-localhost/android-capsule:1}"
REBUILD=0
SKIP_PACKAGES=0
SKIP_IMAGE=0
SKIP_SMOKE=0
ENABLE_LINGER=0
ANDROID_CLI_TOOLS="${ANDROID_CLI_TOOLS:-16111833}"
ANDROID_API="${ANDROID_API:-34}"
ANDROID_IMAGE="${ANDROID_IMAGE:-system-images;android-34;google_apis;x86_64}"
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

  --image NAME        Android image tag (default: $IMAGE)
  --rebuild           Rebuild the image even if it exists
  --skip-packages     Do not install Podman/uidmap/adb
  --skip-image        Do not build the Android image
  --skip-smoke        Do not run the emulator smoke test
  --linger            Enable systemd lingering for this user
  -h, --help          Show this help

Environment:
  BASE_IMAGE=...      Debian base image; use a registry mirror if Docker Hub is unreachable
  DEBIAN_MIRROR=...   Debian apt mirror host (default: deb.debian.org)
  ANDROID_CLI_TOOLS=...  Android command-line tools build (default: $ANDROID_CLI_TOOLS)
  ANDROID_API=...        Android API level (default: $ANDROID_API)
  ANDROID_IMAGE=...      System image package (default: $ANDROID_IMAGE)
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --image) IMAGE="$2"; shift 2 ;;
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

echo "==> android-capsule host setup"
echo "    user:    $USER_NAME"
echo "    image:   $IMAGE"
echo "    API:     $ANDROID_API"

if [[ ! -r /dev/kvm || ! -w /dev/kvm ]]; then
    cat >&2 <<EOF
/dev/kvm is not readable/writable by $USER_NAME.

Either add an ACL for this session:
  sudo setfacl -m u:$USER_NAME:rw /dev/kvm

or add yourself to the kvm group and log in again:
  sudo usermod -aG kvm $USER_NAME
EOF
    exit 1
fi
echo "    /dev/kvm: accessible"

if ! command -v sudo >/dev/null 2>&1; then
    echo "sudo is required." >&2
    exit 1
fi
sudo -v

if [[ "$SKIP_PACKAGES" -eq 0 ]]; then
    if command -v apt-get >/dev/null 2>&1; then
        echo "==> installing Podman and adb"
        sudo apt-get update
        sudo apt-get install -y podman uidmap slirp4netns fuse-overlayfs
        if ! command -v adb >/dev/null 2>&1; then
            sudo apt-get install -y adb || sudo apt-get install -y android-tools-adb
        fi
        for optional in containernetworking-plugins passt netavark aardvark-dns; do
            if apt-cache show "$optional" >/dev/null 2>&1; then
                sudo apt-get install -y "$optional"
            fi
        done
    elif command -v dnf >/dev/null 2>&1; then
        echo "==> installing Podman and adb"
        sudo dnf install -y podman shadow-utils android-tools
    else
        echo "Unsupported package manager. Install podman, uidmap and adb manually." >&2
        exit 1
    fi
else
    echo "==> skipping package installation"
fi

if ! command -v podman >/dev/null 2>&1; then
    echo "podman was not found after installation." >&2
    exit 1
fi
if ! command -v adb >/dev/null 2>&1; then
    echo "adb was not found after installation." >&2
    exit 1
fi

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
    if has_subid /etc/subuid "$user"; then need_uid=0; fi
    if has_subid /etc/subgid "$user"; then need_gid=0; fi
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
    if [[ "$need_uid" -eq 1 ]]; then args+=(--add-subuids "${start}-${end}"); fi
    if [[ "$need_gid" -eq 1 ]]; then args+=(--add-subgids "${start}-${end}"); fi
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
echo "==> applying mappings (podman system migrate)"
podman system migrate || true

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

if [[ "$SKIP_IMAGE" -eq 0 ]]; then
    if [[ "$REBUILD" -eq 1 ]] || ! podman image exists "$IMAGE" >/dev/null 2>&1; then
        echo "==> building $IMAGE"
        echo "    (the first build downloads Android SDK, emulator and a system image; several GB)"
        build_args=()
        if [[ -n "${BASE_IMAGE:-}" ]]; then
            build_args+=(--build-arg "BASE_IMAGE=$BASE_IMAGE")
        fi
        if [[ -n "${DEBIAN_MIRROR:-}" ]]; then
            build_args+=(--build-arg "DEBIAN_MIRROR=$DEBIAN_MIRROR")
        fi
        build_args+=(--build-arg "ANDROID_CLI_TOOLS=$ANDROID_CLI_TOOLS")
        build_args+=(--build-arg "ANDROID_API=$ANDROID_API")
        build_args+=(--build-arg "ANDROID_IMAGE=$ANDROID_IMAGE")
        podman build "${build_args[@]}" \
            -t "$IMAGE" \
            -f images/android-capsule/Containerfile \
            .
    else
        echo "==> image $IMAGE already exists (use --rebuild to rebuild)"
    fi
else
    echo "==> skipping image build"
fi

if [[ "$SKIP_SMOKE" -eq 0 ]]; then
    if ! podman image exists "$IMAGE" >/dev/null 2>&1; then
        echo "==> skipping smoke test: image $IMAGE is missing"
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
        NAME="android-capsule-smoke-$$"
        SERIAL="127.0.0.1:$PORT"
        echo "==> smoke test on $SERIAL (boot can take 1-3 minutes)"
        CID="$(podman run -d --rm --name "$NAME" \
            --device /dev/kvm \
            --shm-size 2g \
            --tmpfs /tmp:rw,size=2g \
            --tmpfs /run:rw,size=64m \
            --memory 4096m \
            --cpus 2 \
            --pids-limit 1024 \
            --cap-drop ALL \
            --security-opt no-new-privileges \
            -p "127.0.0.1:${PORT}:5555" \
            "$IMAGE")"
        SMOKE_CONTAINERS+=("$CID")
        adb disconnect "$SERIAL" >/dev/null 2>&1 || true
        adb connect "$SERIAL" >/dev/null 2>&1 || true
        READY=0
        for _ in $(seq 1 120); do
            if [[ "$(adb -s "$SERIAL" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r\n')" == "1" ]]; then
                READY=1
                break
            fi
            sleep 2
        done
        if [[ "$READY" -eq 1 ]]; then
            echo "    Android boot completed"
            if adb -s "$SERIAL" exec-out screencap -p >"$TMP_DIR/smoke.png" 2>/dev/null \
                && grep -q 'PNG' "$TMP_DIR/smoke.png"; then
                echo "    screencap OK"
            else
                echo "    screencap failed" >&2
                podman logs "$CID" >&2 || true
                exit 1
            fi
        else
            echo "    Android did not finish booting; logs:" >&2
            podman logs "$CID" >&2 || true
            exit 1
        fi
        adb disconnect "$SERIAL" >/dev/null 2>&1 || true
        podman rm -f "$CID" >/dev/null 2>&1 || true
    fi
fi

if [[ "$ENABLE_LINGER" -eq 1 ]]; then
    echo "==> enabling systemd lingering for $USER_NAME"
    sudo loginctl enable-linger "$USER_NAME"
fi

cat <<EOF

==> done

Next steps:
  cargo run --bin androidcapsule -- demo

The demo creates the emulator, saves /tmp/android-capsule-before.png, launches
the Settings app, saves /tmp/android-capsule-after.png, and removes the
container. The library entry point is android_capsule::AndroidCapsule::create.
EOF
