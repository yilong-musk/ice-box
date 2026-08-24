#!/usr/bin/env bash
# Download pinned sing-box into third_party/sing-box/<target>/
# Usage: scripts/fetch-singbox.sh [host | mac-arm64 | mac-x64 | win]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PLATFORM="${1:-host}"
VERSION_FILE="$ROOT/third_party/sing-box/VERSION"
VERSION="$(tr -d '[:space:]' <"$VERSION_FILE")"
OUT_ROOT="$ROOT/third_party/sing-box"

# shellcheck source=scripts/platform.sh
source "$ROOT/scripts/platform.sh"
ice_resolve_platform "$PLATFORM"

case "$ICE_PLATFORM_ALIAS" in
  mac-arm64)
    ASSET="sing-box-${VERSION}-darwin-arm64.tar.gz"
    ;;
  mac-x64)
    ASSET="sing-box-${VERSION}-darwin-amd64.tar.gz"
    ;;
  win)
    ASSET="sing-box-${VERSION}-windows-amd64.zip"
    ;;
esac

URL="https://github.com/SagerNet/sing-box/releases/download/v${VERSION}/${ASSET}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "Fetching $ICE_PLATFORM_LABEL: $URL"
curl -fsSL -o "$TMP/sb.archive" "$URL"

CHECKSUMS="$OUT_ROOT/CHECKSUMS.sha256"
if [[ ! -f "$CHECKSUMS" ]]; then
  echo "missing $CHECKSUMS (expected pinned SHA-256 list)" >&2
  exit 1
fi
EXPECTED="$(awk -v asset="$ASSET" '$2 == asset { print $1; exit }' "$CHECKSUMS")"
if [[ -z "$EXPECTED" ]]; then
  echo "no SHA-256 entry for $ASSET in $CHECKSUMS" >&2
  exit 1
fi
if command -v shasum >/dev/null 2>&1; then
  SHA256_CMD="shasum -a 256"
elif command -v sha256sum >/dev/null 2>&1; then
  SHA256_CMD="sha256sum"
else
  echo "no SHA-256 tool (shasum/sha256sum) available" >&2
  exit 1
fi
# shellcheck disable=SC2086
ACTUAL="$($SHA256_CMD "$TMP/sb.archive" | awk '{ print $1 }')"
if [[ "$ACTUAL" != "$EXPECTED" ]]; then
  echo "SHA-256 mismatch for $ASSET" >&2
  echo "  expected: $EXPECTED" >&2
  echo "  actual:   $ACTUAL" >&2
  exit 1
fi
echo "SHA-256 ok: $ASSET"

dest="$OUT_ROOT/$ICE_TARGET_DIR"
mkdir -p "$dest"

case "$ASSET" in
  *.tar.gz)
    tar -xzf "$TMP/sb.archive" -C "$TMP"
    src="$(find "$TMP" -type f -name "$ICE_BIN" | head -n 1)"
    ;;
  *.zip)
    unzip -q "$TMP/sb.archive" -d "$TMP"
    src="$(find "$TMP" -type f -name "$ICE_BIN" | head -n 1)"
    ;;
esac

if [[ -z "${src:-}" || ! -f "$src" ]]; then
  echo "extracted binary not found" >&2
  exit 1
fi

cp "$src" "$dest/$ICE_BIN"
chmod +x "$dest/$ICE_BIN" 2>/dev/null || true
echo "Installed $dest/$ICE_BIN"
"$dest/$ICE_BIN" version
