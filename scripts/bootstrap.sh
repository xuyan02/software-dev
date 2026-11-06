#!/usr/bin/env bash
set -euo pipefail

echo "==> installing Rust toolchain"
rustup toolchain install 1.93.1 --profile minimal
rustup component add rustfmt clippy

echo "==> installing common cargo tools (best effort)"
cargo install --locked cargo-nextest cargo-deny cargo-audit sqlx-cli || true

echo "==> done"
echo "Run: scripts/check-all.sh"
