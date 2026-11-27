#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Verify a packaged cross-platform CLI archive (.tar.gz or .zip):
#   - canonical filename (`ice-box-cli_<version>_<os>_<arch>.<tar.gz|zip>`)
#   - required payload files for the target OS
#   - executable permissions on Unix binaries
#   - Linux glibc symbol ceiling (<= 2.35 by default) via scripts/check-glibc-floor.sh
#   - host-native smoke check (`--version`, `tools`) when executable on this host
#
# Usage:
#   scripts/verify-cli-archive.sh --archive <path> --platform <alias> \
#     [--version <ver>] [--glibc-max <ver>] [--glibc-script <path>] [--skip-smoke]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ARCHIVE=""
PLATFORM=""
VERSION=""
GLIBC_MAX="2.35"
GLIBC_SCRIPT="$ROOT/scripts/check-glibc-floor.sh"
SKIP_SMOKE=0

usage() {
  cat >&2 <<'EOF'
usage: scripts/verify-cli-archive.sh --archive <path> \
  --platform <mac-arm64|mac-x64|win|linux-x64|linux-arm64> \
  [--version <ver>] [--glibc-max <ver>] [--glibc-script <path>] [--skip-smoke]
EOF
}

while (($# > 0)); do
  case "$1" in
  --archive)
    ARCHIVE="${2:-}"
    shift 2
    ;;
  --platform)
    PLATFORM="${2:-}"
    shift 2
    ;;
  --version)
    VERSION="${2:-}"
    shift 2
    ;;
  --glibc-max)
    GLIBC_MAX="${2:-}"
    shift 2
    ;;
  --glibc-script)
    GLIBC_SCRIPT="${2:-}"
    shift 2
    ;;
  --skip-smoke)
    SKIP_SMOKE=1
    shift
    ;;
  -h | --help)
    usage
    exit 0
    ;;
  *)
    echo "verify-cli-archive: unknown argument: $1" >&2
    usage
    exit 2
    ;;
  esac
done

if [[ -z "$ARCHIVE" || -z "$PLATFORM" ]]; then
  echo "verify-cli-archive: --archive and --platform are required" >&2
  usage
  exit 2
fi

if [[ ! -s "$ARCHIVE" ]]; then
  echo "verify-cli-archive: missing or empty archive: $ARCHIVE" >&2
  exit 1
fi

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

case "$ICE_PLATFORM_ALIAS" in
mac-arm64) ARCHIVE_TARGET="darwin_aarch64"; EXT="tar.gz"; CLI_BIN="ice-box-cli" ;;
mac-x64) ARCHIVE_TARGET="darwin_x86_64"; EXT="tar.gz"; CLI_BIN="ice-box-cli" ;;
win) ARCHIVE_TARGET="windows_x86_64"; EXT="zip"; CLI_BIN="ice-box-cli.exe" ;;
linux-x64) ARCHIVE_TARGET="linux_x86_64"; EXT="tar.gz"; CLI_BIN="ice-box-cli" ;;
linux-arm64) ARCHIVE_TARGET="linux_aarch64"; EXT="tar.gz"; CLI_BIN="ice-box-cli" ;;
esac

STEM="ice-box-cli_${VERSION}_${ARCHIVE_TARGET}"
EXPECTED_NAME="${STEM}.${EXT}"
ACTUAL_NAME="$(basename "$ARCHIVE")"
if [[ "$ACTUAL_NAME" != "$EXPECTED_NAME" ]]; then
  echo "verify-cli-archive: unexpected filename '$ACTUAL_NAME' (expected '$EXPECTED_NAME')" >&2
  exit 1
fi

WORK="$(mktemp -d "${TMPDIR:-/tmp}/ice-box-cli-verify-XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

if [[ "$EXT" == "tar.gz" ]]; then
  tar -xzf "$ARCHIVE" -C "$WORK"
else
  python3 - "$ARCHIVE" "$WORK" <<'PY'
import sys, zipfile
with zipfile.ZipFile(sys.argv[1], "r") as zf:
    zf.extractall(sys.argv[2])
PY
fi

EXTRACT="$WORK/$STEM"
if [[ ! -d "$EXTRACT" ]]; then
  echo "verify-cli-archive: archive did not extract expected root directory '$STEM'" >&2
  exit 1
fi

REQUIRED_FILES=(
  "$CLI_BIN"
  "resources/$ICE_BIN"
  "resources/geoip/geoip-cn.srs"
  "resources/geoip/geosite-cn.srs"
  "LICENSE"
  "NOTICE"
  "sing-box-LICENSE"
)
REQUIRED_EXEC=()

case "$ICE_PLATFORM_ALIAS" in
mac-arm64 | mac-x64 | linux-x64 | linux-arm64)
  REQUIRED_FILES+=("resources/ice-helper")
  REQUIRED_EXEC+=("$CLI_BIN" "resources/$ICE_BIN" "resources/ice-helper")
  ;;
win)
  REQUIRED_FILES+=("resources/libcronet.dll" "resources/ice-tun-launcher.exe")
  ;;
esac

for rel in "${REQUIRED_FILES[@]}"; do
  if [[ ! -f "$EXTRACT/$rel" || ! -s "$EXTRACT/$rel" ]]; then
    echo "verify-cli-archive: missing or empty '$rel' in $ARCHIVE" >&2
    exit 1
  fi
done

for rel in ${REQUIRED_EXEC[@]+"${REQUIRED_EXEC[@]}"}; do
  if [[ ! -x "$EXTRACT/$rel" ]]; then
    echo "verify-cli-archive: non-executable '$rel' in $ARCHIVE" >&2
    exit 1
  fi
done

if [[ "$ICE_PLATFORM_ALIAS" == linux-* && "$(uname -s)" == Linux ]]; then
  bash "$GLIBC_SCRIPT" "$GLIBC_MAX" \
    "$EXTRACT/$CLI_BIN" \
    "$EXTRACT/resources/ice-helper" \
    "$EXTRACT/resources/$ICE_BIN"
fi

HOST_PLATFORM="$(ice_host_platform || true)"
if [[ "$SKIP_SMOKE" -eq 0 && "$ICE_PLATFORM_ALIAS" == "$HOST_PLATFORM" ]]; then
  "$EXTRACT/$CLI_BIN" --version >/dev/null
  "$EXTRACT/$CLI_BIN" tools >/dev/null
fi

echo "verified CLI archive: $ARCHIVE"
