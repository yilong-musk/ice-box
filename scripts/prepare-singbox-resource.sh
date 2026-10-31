#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
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

# A binary that cannot run on this host (cross-arch / cross-OS) prints nothing.
probe_version() {
  "$1" version 2>/dev/null | head -n 1 || true
}

# Never stage a core that does not match the pin. The generator emits the
# pinned core's schema (for example `disable_chrome_parrot`, which 1.13
# rejects), so a stale artifact only fails later at core startup with an
# "unknown field" decode error. Fetch the pinned build when missing or stale.
needs_fetch=false
if [[ ! -f "$SRC" ]]; then
  echo "Missing $SRC — fetching pinned $VERSION" >&2
  needs_fetch=true
else
  src_version="$(probe_version "$SRC")"
  if [[ -z "$src_version" ]]; then
    echo "note: cannot execute $SRC on this host; skipping version check" >&2
  elif [[ "$src_version" != *"$VERSION"* ]]; then
    echo "Stale $SRC ($src_version) — fetching pinned $VERSION" >&2
    needs_fetch=true
  fi
fi

if $needs_fetch; then
  bash "$ROOT/scripts/fetch-singbox.sh" "$ICE_PLATFORM_ALIAS"
fi

if [[ ! -f "$SRC" ]]; then
  echo "Missing $SRC — run: npm run fetch-singbox -- $ICE_PLATFORM_ALIAS (pin $VERSION)" >&2
  exit 1
fi

mkdir -p "$DEST_DIR"
cp "$SRC" "$DEST"
chmod +x "$DEST" 2>/dev/null || true

got="$(probe_version "$DEST")"
echo "Prepared resource $DEST for $ICE_PLATFORM_LABEL ($got)"
if [[ -n "$got" && "$got" != *"$VERSION"* ]]; then
  echo "error: staged core reports '$got', expected pin $VERSION" >&2
  exit 1
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

# Privileged helper daemon (macOS): embedded into the bundle so
# the installer/release pipeline can install it into /Library/PrivilegedHelperTools.
case "$ICE_PLATFORM_ALIAS" in
  mac-arm64 | mac-x64)
    HELPER_DEST="$DEST_DIR/ice-helper"
    if cargo build --release -p ice-helper --manifest-path "$ROOT/Cargo.toml" >/dev/null 2>&1; then
      cp "$ROOT/target/release/ice-helper" "$HELPER_DEST"
      chmod +x "$HELPER_DEST"
      echo "Prepared helper resource $HELPER_DEST (macOS privileged helper daemon)"
    else
      echo "error: cargo build -p ice-helper failed; run it manually before the release build" >&2
      exit 1
    fi
    ;;
  win)
    # Windows archive companions (Windows TUN packaging): the
    # NaiveProxy outbound needs libcronet.dll next to sing-box.exe. wintun.dll
    # is embedded in the pinned binary — the T0 spike re-verifies this before
    # the Windows TUN gate flips.
    for companion in libcronet.dll; do
      COMPANION_SRC="$ROOT/third_party/sing-box/$ICE_TARGET_DIR/$companion"
      if [[ -f "$COMPANION_SRC" ]]; then
        cp "$COMPANION_SRC" "$DEST_DIR/$companion"
        echo "Prepared resource $DEST_DIR/$companion (Windows archive companion)"
      else
        echo "warning: $COMPANION_SRC missing — run scripts/fetch-singbox.sh win (NaiveProxy outbound will fail at runtime)" >&2
      fi
    done
    ;;
esac
