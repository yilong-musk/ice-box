#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Fixture tests for scripts/verify-cli-archive.sh across macOS, Windows, and Linux.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SCRIPT="$ROOT/scripts/verify-cli-archive.sh"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/ice-box-test-cli-archive-XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

FAKE_GLIBC="$TMP/fake-glibc.sh"
cat >"$FAKE_GLIBC" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${FAIL_GLIBC:-0}" == "1" ]]; then
  echo "check-glibc-floor: FAIL mock glibc ceiling exceeded" >&2
  exit 1
fi
exit 0
EOF
chmod +x "$FAKE_GLIBC"

expect_ok() {
  local name="$1"
  shift
  if ! "$@" >/dev/null 2>&1; then
    echo "test-verify-cli-archive: $name: expected success" >&2
    "$@" >&2 || true
    exit 1
  fi
}

expect_fail() {
  local name="$1"
  local needle="$2"
  shift 2
  local out
  if out="$("$@" 2>&1)"; then
    echo "test-verify-cli-archive: $name: expected failure" >&2
    exit 1
  fi
  if ! printf '%s\n' "$out" | grep -Fq "$needle"; then
    echo "test-verify-cli-archive: $name: expected '$needle' in:" >&2
    printf '%s\n' "$out" >&2
    exit 1
  fi
}

make_tar_archive() {
  local dest="$1"
  local stem="$2"
  local omit="${3:-}"
  local noexec="${4:-}"
  local work="$TMP/work-$RANDOM"
  local stage="$work/$stem"
  mkdir -p "$stage/resources/geoip"
  printf '#!/bin/sh\necho ok\n' >"$stage/ice-box-cli"
  printf 'sb\n' >"$stage/resources/sing-box"
  printf 'hp\n' >"$stage/resources/ice-helper"
  printf 'cn\n' >"$stage/resources/geoip/geoip-cn.srs"
  printf 'gs\n' >"$stage/resources/geoip/geosite-cn.srs"
  printf 'lic\n' >"$stage/LICENSE"
  printf 'not\n' >"$stage/NOTICE"
  printf 'sbl\n' >"$stage/sing-box-LICENSE"
  chmod +x "$stage/ice-box-cli" "$stage/resources/sing-box" "$stage/resources/ice-helper"
  if [[ -n "$omit" ]]; then
    rm -f "$stage/$omit"
  fi
  if [[ -n "$noexec" ]]; then
    chmod -x "$stage/$noexec"
  fi
  tar -czf "$dest" -C "$work" "$stem"
}

make_zip_archive() {
  local dest="$1"
  local stem="$2"
  local omit="${3:-}"
  local work="$TMP/work-$RANDOM"
  local stage="$work/$stem"
  mkdir -p "$stage/resources/geoip"
  printf 'cli\n' >"$stage/ice-box-cli.exe"
  printf 'sb\n' >"$stage/resources/sing-box.exe"
  printf 'dll\n' >"$stage/resources/libcronet.dll"
  printf 'ln\n' >"$stage/resources/ice-tun-launcher.exe"
  printf 'cn\n' >"$stage/resources/geoip/geoip-cn.srs"
  printf 'gs\n' >"$stage/resources/geoip/geosite-cn.srs"
  printf 'lic\n' >"$stage/LICENSE"
  printf 'not\n' >"$stage/NOTICE"
  printf 'sbl\n' >"$stage/sing-box-LICENSE"
  if [[ -n "$omit" ]]; then
    rm -f "$stage/$omit"
  fi
  python3 - "$work" "$stem" "$dest" <<'PY'
import sys, zipfile
from pathlib import Path
work, stem, dest = Path(sys.argv[1]), sys.argv[2], Path(sys.argv[3])
with zipfile.ZipFile(dest, "w", compression=zipfile.ZIP_DEFLATED) as zf:
    for p in sorted((work / stem).rglob("*")):
        if p.is_file():
            zf.write(p, p.relative_to(work).as_posix())
PY
}

VER="0.5.0"

echo "== happy paths (linux, macOS, Windows) =="
make_tar_archive "$TMP/ice-box-cli_${VER}_linux_x86_64.tar.gz" "ice-box-cli_${VER}_linux_x86_64"
expect_ok "linux-x64" bash "$SCRIPT" --archive "$TMP/ice-box-cli_${VER}_linux_x86_64.tar.gz" \
  --platform linux-x64 --version "$VER" --glibc-script "$FAKE_GLIBC" --skip-smoke

make_tar_archive "$TMP/ice-box-cli_${VER}_darwin_aarch64.tar.gz" "ice-box-cli_${VER}_darwin_aarch64"
expect_ok "mac-arm64" bash "$SCRIPT" --archive "$TMP/ice-box-cli_${VER}_darwin_aarch64.tar.gz" \
  --platform mac-arm64 --version "$VER" --skip-smoke

make_zip_archive "$TMP/ice-box-cli_${VER}_windows_x86_64.zip" "ice-box-cli_${VER}_windows_x86_64"
expect_ok "win-x64" bash "$SCRIPT" --archive "$TMP/ice-box-cli_${VER}_windows_x86_64.zip" \
  --platform win --version "$VER" --skip-smoke

echo "== failure cases =="
make_tar_archive "$TMP/ice-box-cli_${VER}_linux_aarch64.tar.gz" "ice-box-cli_${VER}_linux_aarch64" "resources/ice-helper"
expect_fail "missing-helper" "resources/ice-helper" \
  bash "$SCRIPT" --archive "$TMP/ice-box-cli_${VER}_linux_aarch64.tar.gz" \
  --platform linux-arm64 --version "$VER" --glibc-script "$FAKE_GLIBC" --skip-smoke

make_tar_archive "$TMP/noexec-linux.tar.gz" "ice-box-cli_${VER}_linux_x86_64" "" "resources/sing-box"
cp "$TMP/noexec-linux.tar.gz" "$TMP/ice-box-cli_${VER}_linux_x86_64.tar.gz"
expect_fail "non-exec-singbox" "non-executable 'resources/sing-box'" \
  bash "$SCRIPT" --archive "$TMP/ice-box-cli_${VER}_linux_x86_64.tar.gz" \
  --platform linux-x64 --version "$VER" --glibc-script "$FAKE_GLIBC" --skip-smoke

make_zip_archive "$TMP/ice-box-cli_${VER}_windows_x86_64.zip" "ice-box-cli_${VER}_windows_x86_64" "resources/libcronet.dll"
expect_fail "win-missing-cronet" "resources/libcronet.dll" \
  bash "$SCRIPT" --archive "$TMP/ice-box-cli_${VER}_windows_x86_64.zip" \
  --platform win --version "$VER" --skip-smoke

make_tar_archive "$TMP/wrong-name.tar.gz" "ice-box-cli_${VER}_linux_x86_64"
expect_fail "wrong-name" "unexpected filename" \
  bash "$SCRIPT" --archive "$TMP/wrong-name.tar.gz" \
  --platform linux-x64 --version "$VER" --glibc-script "$FAKE_GLIBC" --skip-smoke

if [[ "$(uname -s)" == Linux ]]; then
  make_tar_archive "$TMP/ice-box-cli_${VER}_linux_x86_64.tar.gz" "ice-box-cli_${VER}_linux_x86_64"
  expect_fail "glibc-floor" "mock glibc ceiling exceeded" \
    env FAIL_GLIBC=1 bash "$SCRIPT" --archive "$TMP/ice-box-cli_${VER}_linux_x86_64.tar.gz" \
    --platform linux-x64 --version "$VER" --glibc-script "$FAKE_GLIBC" --skip-smoke
fi

echo "test-verify-cli-archive: OK"
