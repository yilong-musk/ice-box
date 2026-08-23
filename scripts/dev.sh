#!/usr/bin/env bash
# Start Tauri dev for a specific platform target.
# Usage: scripts/dev.sh [host | mac-arm64 | mac-x64 | win]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PLATFORM="${1:-host}"

# shellcheck source=scripts/platform.sh
source "$ROOT/scripts/platform.sh"
ice_resolve_platform "$PLATFORM"
ice_assert_dev_host

echo "== ice-box dev: $ICE_PLATFORM_LABEL =="

bash "$ROOT/scripts/prepare-singbox-resource.sh" "$ICE_PLATFORM_ALIAS"

cd "$ROOT/apps/desktop"

TAURI_ARGS=(dev)
CARGO_TARGET="$(ice_dev_cargo_target || true)"
if [[ -n "${CARGO_TARGET:-}" ]]; then
  echo "cargo target: $CARGO_TARGET"
  TAURI_ARGS+=(--target "$CARGO_TARGET")
fi

exec npm run tauri -- "${TAURI_ARGS[@]}"
