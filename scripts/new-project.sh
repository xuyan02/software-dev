#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
NAME="${1:-}"

if [[ -z "$NAME" ]]; then
  echo "usage: $0 <project-name>" >&2
  exit 1
fi

if [[ ! "$NAME" =~ ^[a-z][a-z0-9-]*$ ]]; then
  echo "error: name must be kebab-case (got '$NAME')" >&2
  exit 1
fi

CRATE_DIR="projects/$NAME/crates/$NAME"
DEST="$ROOT/$CRATE_DIR"
if [[ -e "$DEST" ]]; then
  echo "error: $DEST already exists" >&2
  exit 1
fi

mkdir -p "$ROOT/projects/$NAME/crates"
cp -R "$ROOT/shared/templates/rust-service" "$DEST"
grep -rl '__PROJECT__' "$DEST" | xargs sed -i "s/__PROJECT__/$NAME/g"

# Cargo forbids nested workspaces, so every Rust crate is a member of the root
# workspace. Insert the new member before the marker line.
sed -i "/# new projects: insert member below this line/i\\    \"$CRATE_DIR\"," "$ROOT/Cargo.toml"

echo "created $DEST"
echo "next: add it to docs/projects.md"
