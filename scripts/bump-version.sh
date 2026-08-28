#!/usr/bin/env bash
# Bump the ice-box version across every version source and refresh Cargo.lock.
# Usage: scripts/bump-version.sh 0.1.1
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

NEW_VERSION="${1:-}"
if [[ ! "$NEW_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]; then
  echo "usage: $0 <semver, e.g. 0.1.1>" >&2
  exit 1
fi

CARGO_TOML="Cargo.toml"
PACKAGE_JSON="apps/desktop/package.json"
TAURI_CONF="apps/desktop/src-tauri/tauri.conf.json"

bump_key() {
  local file="$1"
  local pattern="$2"
  if ! grep -q "^${pattern}\"[0-9][^\"]*\"" "$file"; then
    echo "pattern '$pattern' not found in $file" >&2
    exit 1
  fi
  PATTERN="$pattern" NEW_VERSION="$NEW_VERSION" perl -pi \
    -e 's/^($ENV{PATTERN})"[0-9][^"]*"/$1"$ENV{NEW_VERSION}"/' "$file"
}

bump_key "$CARGO_TOML" 'version = '
bump_key "$PACKAGE_JSON" '  "version": '
bump_key "$TAURI_CONF" '  "version": '

echo "bumped to $NEW_VERSION:"
grep -n '^version' "$CARGO_TOML"
grep -n '"version"' "$PACKAGE_JSON"
grep -n '"version"' "$TAURI_CONF"

echo "== refreshing Cargo.lock =="
cargo check --workspace --quiet

echo "bump-version: OK ($NEW_VERSION)"