#!/usr/bin/env bash
# Ensure src-tauri/resources has the sing-box binary for the given platform.
# Usage: scripts/prepare-singbox-resource.sh [host | mac-arm64 | mac-x64 | win]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PLATFORM="${1:-host}"
VERSION="$(tr -d '[:space:]' <"$ROOT/third_party/sing-box/VERSION")"
ST="$ROOT/apps/desktop/src-tauri"

# shellcheck source=scripts/platform.sh
source "$ROOT/scripts/platform.sh"
ice_resolve_platform "$PLATFORM"

SRC="$ROOT/third_party/sing-box/$ICE_TARGET_DIR/$ICE_BIN"
DEST_DIR="$ST/resources"
DEST="$DEST_DIR/$ICE_BIN"

if [[ ! -f "$SRC" ]]; then
  echo "Missing $SRC — run: npm run fetch-singbox -- $ICE_PLATFORM_ALIAS (pin $VERSION)" >&2
  exit 1
fi

mkdir -p "$DEST_DIR"
cp "$SRC" "$DEST"
chmod +x "$DEST" 2>/dev/null || true

got="$("$DEST" version 2>/dev/null | head -n 1 || true)"
echo "Prepared resource $DEST for $ICE_PLATFORM_LABEL ($got)"
if [[ "$got" != *"$VERSION"* ]]; then
  echo "warning: binary version string does not contain pin $VERSION: $got" >&2
fi

GEOIP_SRC="$ROOT/third_party/sing-geoip/rule-set"
GEOIP_DEST="$DEST_DIR/geoip"
if [[ -d "$GEOIP_SRC" ]]; then
  mkdir -p "$GEOIP_DEST"
  cp "$GEOIP_SRC"/*.srs "$GEOIP_DEST"/
  echo "Prepared $GEOIP_DEST ($(ls "$GEOIP_DEST" | wc -l | tr -d ' ') geoip rule-sets)"
else
  echo "error: missing $GEOIP_SRC — run scripts/fetch-geoip.sh (packaged app would silently drop GEOIP rules)" >&2
  exit 1
fi
