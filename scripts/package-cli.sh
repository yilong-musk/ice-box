#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Build and package a self-contained cross-platform CLI archive for ice-box.
#
# Usage:
#   scripts/package-cli.sh [--platform <host|mac-arm64|mac-x64|win|linux-x64|linux-arm64>] \
#     [--out-dir <dir>] [--version <ver>] [--skip-verify]
#
# Archive names:
#   mac-arm64   -> ice-box-cli_<version>_darwin_aarch64.tar.gz
#   mac-x64     -> ice-box-cli_<version>_darwin_x86_64.tar.gz
#   win         -> ice-box-cli_<version>_windows_x86_64.zip
#   linux-x64   -> ice-box-cli_<version>_linux_x86_64.tar.gz
#   linux-arm64 -> ice-box-cli_<version>_linux_aarch64.tar.gz
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PLATFORM="host"
OUT_DIR="$ROOT/target/release/cli-dist"
VERSION=""
SKIP_VERIFY=0

usage() {
  cat >&2 <<'EOF'
usage: scripts/package-cli.sh \
  [--platform <host|mac-arm64|mac-x64|win|linux-x64|linux-arm64>] \
  [--out-dir <dir>] [--version <ver>] [--skip-verify]
EOF
}

while (($# > 0)); do
  case "$1" in
  --platform)
    PLATFORM="${2:-}"
    shift 2
    ;;
  --out-dir)
    OUT_DIR="${2:-}"
    shift 2
    ;;
  --version)
    VERSION="${2:-}"
    shift 2
    ;;
  --skip-verify)
    SKIP_VERIFY=1
    shift
    ;;
  -h | --help)
    usage
    exit 0
    ;;
  *)
    echo "package-cli: unknown argument: $1" >&2
    usage
    exit 2
    ;;
  esac
done

# shellcheck source=scripts/platform.sh
source "$ROOT/scripts/platform.sh"
ice_resolve_platform "$PLATFORM"

if [[ -z "$VERSION" ]]; then
  VERSION="$(python3 -c '
import re, sys
text = open(sys.argv[1], encoding="utf-8").read()
m = re.search(r"\[workspace\.package\][\s\S]*?^version\s*=\s*\"([^\"]+)\"", text, re.M)
assert m, "workspace version not found"
print(m.group(1))
' "$ROOT/Cargo.toml")"
fi
VERSION="${VERSION#v}"

SINGBOX_SRC="$ROOT/third_party/sing-box/$ICE_TARGET_DIR/$ICE_BIN"
if [[ ! -f "$SINGBOX_SRC" ]]; then
  echo "package-cli: missing $SINGBOX_SRC — run scripts/fetch-singbox.sh $ICE_PLATFORM_ALIAS" >&2
  exit 1
fi

GEOIP_SRC="$ROOT/third_party/bundled-rule-sets/rule-set"
if [[ ! -f "$GEOIP_SRC/geoip-cn.srs" || ! -f "$GEOIP_SRC/geosite-cn.srs" ]]; then
  echo "package-cli: missing bundled rule-sets under $GEOIP_SRC — run scripts/fetch-geoip.sh" >&2
  exit 1
fi

CARGO_TARGET="$(ice_dev_cargo_target || true)"
CARGO_TARGET_ARGS=()
TARGET_SUBDIR="target/release"
if [[ -n "$CARGO_TARGET" ]]; then
  CARGO_TARGET_ARGS=(--target "$CARGO_TARGET")
  TARGET_SUBDIR="target/$CARGO_TARGET/release"
fi

echo "== building ice-cli ($ICE_PLATFORM_LABEL) =="
cargo build --release -p ice-cli "${CARGO_TARGET_ARGS[@]}" --manifest-path "$ROOT/Cargo.toml"

case "$ICE_PLATFORM_ALIAS" in
mac-arm64 | mac-x64 | linux-x64 | linux-arm64)
  echo "== building ice-helper ($ICE_PLATFORM_LABEL) =="
  cargo build --release -p ice-helper "${CARGO_TARGET_ARGS[@]}" --manifest-path "$ROOT/Cargo.toml"
  ;;
win)
  echo "== building ice-tun-launcher ($ICE_PLATFORM_LABEL) =="
  cargo build --release -p ice-tun-launcher "${CARGO_TARGET_ARGS[@]}" --manifest-path "$ROOT/Cargo.toml"
  ;;
esac

case "$ICE_PLATFORM_ALIAS" in
mac-arm64) ARCHIVE_TARGET="darwin_aarch64"; EXT="tar.gz"; CLI_BIN="ice-box-cli" ;;
mac-x64) ARCHIVE_TARGET="darwin_x86_64"; EXT="tar.gz"; CLI_BIN="ice-box-cli" ;;
win) ARCHIVE_TARGET="windows_x86_64"; EXT="zip"; CLI_BIN="ice-box-cli.exe" ;;
linux-x64) ARCHIVE_TARGET="linux_x86_64"; EXT="tar.gz"; CLI_BIN="ice-box-cli" ;;
linux-arm64) ARCHIVE_TARGET="linux_aarch64"; EXT="tar.gz"; CLI_BIN="ice-box-cli" ;;
esac

STEM="ice-box-cli_${VERSION}_${ARCHIVE_TARGET}"
ARCHIVE_PATH="$OUT_DIR/${STEM}.${EXT}"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/ice-box-cli-pkg-XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

STAGE="$WORK/$STEM"
mkdir -p "$STAGE/resources/geoip"

cp "$ROOT/$TARGET_SUBDIR/$CLI_BIN" "$STAGE/$CLI_BIN"
cp "$SINGBOX_SRC" "$STAGE/resources/$ICE_BIN"
cp "$GEOIP_SRC"/*.srs "$STAGE/resources/geoip/"
cp "$ROOT/LICENSE" "$STAGE/LICENSE"
cp "$ROOT/NOTICE" "$STAGE/NOTICE"
cp "$ROOT/third_party/sing-box/LICENSE" "$STAGE/sing-box-LICENSE"

case "$ICE_PLATFORM_ALIAS" in
mac-arm64 | mac-x64 | linux-x64 | linux-arm64)
  cp "$ROOT/$TARGET_SUBDIR/ice-helper" "$STAGE/resources/ice-helper"
  chmod +x "$STAGE/$CLI_BIN" "$STAGE/resources/$ICE_BIN" "$STAGE/resources/ice-helper"
  ;;
win)
  cp "$ROOT/$TARGET_SUBDIR/ice-tun-launcher.exe" "$STAGE/resources/ice-tun-launcher.exe"
  CRONET_SRC="$ROOT/third_party/sing-box/$ICE_TARGET_DIR/libcronet.dll"
  if [[ ! -f "$CRONET_SRC" ]]; then
    echo "package-cli: missing $CRONET_SRC (required Windows sing-box companion)" >&2
    exit 1
  fi
  cp "$CRONET_SRC" "$STAGE/resources/libcronet.dll"
  ;;
esac

if [[ "$ICE_PLATFORM_ALIAS" == mac-* ]] && command -v codesign >/dev/null 2>&1; then
  # Match the project's documented ad-hoc signing policy on macOS.
  codesign --force --sign - "$STAGE/$CLI_BIN" "$STAGE/resources/$ICE_BIN" "$STAGE/resources/ice-helper"
fi

mkdir -p "$OUT_DIR"
rm -f "$ARCHIVE_PATH"
if [[ "$EXT" == "tar.gz" ]]; then
  tar -czf "$ARCHIVE_PATH" -C "$WORK" "$STEM"
else
  python3 - "$WORK" "$STEM" "$ARCHIVE_PATH" <<'PY'
import os
import sys
import zipfile
from pathlib import Path

work = Path(sys.argv[1])
stem = sys.argv[2]
archive = Path(sys.argv[3])
stage = work / stem

with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as zf:
    for path in sorted(stage.rglob("*")):
        if path.is_file():
            zf.write(path, path.relative_to(work).as_posix())
PY
fi

echo "packaged CLI archive: $ARCHIVE_PATH"

if [[ "$SKIP_VERIFY" -eq 0 ]]; then
  bash "$ROOT/scripts/verify-cli-archive.sh" \
    --archive "$ARCHIVE_PATH" \
    --platform "$ICE_PLATFORM_ALIAS" \
    --version "$VERSION"
fi
