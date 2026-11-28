#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Offline fixture tests for scripts/install-cli.sh and scripts/install-cli.ps1.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SCRIPT="$ROOT/scripts/install-cli.sh"
PS_SCRIPT="$ROOT/scripts/install-cli.ps1"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/ice-box-test-install-cli-XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

fail() {
  printf 'test-install-cli: %b\n' "$*" >&2
  exit 1
}

expect_fail() {
  local name="$1"
  local needle="$2"
  shift 2
  local out
  if out="$("$@" 2>&1)"; then
    fail "$name: expected failure"
  fi
  if ! printf '%s\n' "$out" | grep -Fq -- "$needle"; then
    fail "$name: expected '$needle' in:\n$out"
  fi
}

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print tolower($1)}'
  else
    shasum -a 256 "$1" | awk '{print tolower($1)}'
  fi
}

make_fixture_release() {
  local dir="$1"
  local ver="$2"
  local target="$3"
  local marker="${4:-ok}"
  local stem="ice-box-cli_${ver}_${target}"
  local archive="${stem}.tar.gz"
  local work="$TMP/stage-${ver}-${RANDOM}"
  local stage="$work/$stem"

  mkdir -p "$dir" "$stage/resources/geoip"
  cat >"$stage/ice-box-cli" <<EOF
#!/bin/sh
echo "ice-box-cli ${ver} (${marker})"
EOF
  printf 'sb\n' >"$stage/resources/sing-box"
  printf 'hp\n' >"$stage/resources/ice-helper"
  printf 'cn\n' >"$stage/resources/geoip/geoip-cn.srs"
  printf 'gs\n' >"$stage/resources/geoip/geosite-cn.srs"
  chmod +x "$stage/ice-box-cli" "$stage/resources/sing-box" "$stage/resources/ice-helper"

  tar -czf "$dir/$archive" -C "$work" "$stem"
  local sum
  sum="$(sha256_file "$dir/$archive")"
  printf '%s  %s\n' "$sum" "$archive" >"$dir/SHA256SUMS"
}

echo "== syntax check =="
bash -n "$SCRIPT"
test -f "$PS_SCRIPT"
grep -Fq 'ice-box-cli_${bareVersion}_windows_x86_64' "$PS_SCRIPT" ||
  fail "ps1: missing canonical Windows archive pattern"
grep -Fq 'SHA256SUMS' "$PS_SCRIPT" ||
  fail "ps1: missing SHA256SUMS verification"
if command -v pwsh >/dev/null 2>&1; then
  pwsh -NoProfile -Command '
    $errors = $null
    [void][System.Management.Automation.Language.Parser]::ParseFile($args[0], [ref]$null, [ref]$errors)
    if ($errors.Count -gt 0) { $errors | ForEach-Object { Write-Error $_.Message }; exit 1 }
  ' "$PS_SCRIPT"
fi

echo "== happy path install + symlink + upgrade + uninstall =="
REL1="$TMP/rel-0.5.0"
make_fixture_release "$REL1" "0.5.0" "linux_x86_64" "v1"

INSTALL_DIR="$TMP/opt/ice-box-cli"
BIN_DIR="$TMP/bin"

out="$(
  ICE_BOX_RELEASE_BASE_URL="file://$REL1" \
    bash "$SCRIPT" \
    --version v0.5.0 \
    --platform linux-x64 \
    --install-dir "$INSTALL_DIR" \
    --bin-dir "$BIN_DIR"
)"
printf '%s\n' "$out" | grep -Fq "ice-box-cli 0.5.0 (v1)" ||
  fail "install-v1: expected smoke output, got:\n$out"
test -L "$BIN_DIR/ice-box-cli" || fail "install-v1: missing symlink"
test -f "$INSTALL_DIR/current/resources/sing-box" || fail "install-v1: missing bundled sing-box"
test -f "$INSTALL_DIR/current/resources/geoip/geoip-cn.srs" || fail "install-v1: missing geoip"

# Leave a stale file in current/ to ensure upgrade replaces it cleanly.
touch "$INSTALL_DIR/current/stale-marker"

REL2="$TMP/rel-0.5.1"
make_fixture_release "$REL2" "0.5.1" "linux_x86_64" "v2"
out2="$(
  ICE_BOX_RELEASE_BASE_URL="file://$REL2" \
    bash "$SCRIPT" \
    --version 0.5.1 \
    --platform linux-x64 \
    --install-dir "$INSTALL_DIR" \
    --bin-dir "$BIN_DIR"
)"
printf '%s\n' "$out2" | grep -Fq "ice-box-cli 0.5.1 (v2)" ||
  fail "upgrade-v2: expected upgraded smoke output, got:\n$out2"
test ! -e "$INSTALL_DIR/current/stale-marker" || fail "upgrade-v2: stale file survived upgrade"

# Verify uninstall removes symlink and install dir.
bash "$SCRIPT" --uninstall --install-dir "$INSTALL_DIR" --bin-dir "$BIN_DIR" >/dev/null
test ! -e "$BIN_DIR/ice-box-cli" || fail "uninstall: symlink not removed"
test ! -e "$INSTALL_DIR" || fail "uninstall: install dir not removed"

echo "== uninstall refuses to delete symlink pointing outside install-dir =="
mkdir -p "$BIN_DIR" "$TMP/other"
printf '#!/bin/sh\necho other\n' >"$TMP/other/ice-box-cli"
chmod +x "$TMP/other/ice-box-cli"
ln -s "$TMP/other/ice-box-cli" "$BIN_DIR/ice-box-cli"
bash "$SCRIPT" --uninstall --install-dir "$INSTALL_DIR" --bin-dir "$BIN_DIR" >/dev/null
test -L "$BIN_DIR/ice-box-cli" || fail "uninstall-foreign: deleted unrelated symlink"
rm -f "$BIN_DIR/ice-box-cli"

echo "== failure cases (tampered checksum, invalid version, invalid repo) =="
TAMPERED="$TMP/rel-tampered"
make_fixture_release "$TAMPERED" "0.5.2" "darwin_aarch64" "bad"
printf 'tamper' >>"$TAMPERED/ice-box-cli_0.5.2_darwin_aarch64.tar.gz"
expect_fail "tampered-sha256" "SHA-256 mismatch" \
  env ICE_BOX_RELEASE_BASE_URL="file://$TAMPERED" \
  bash "$SCRIPT" --version 0.5.2 --platform mac-arm64 --install-dir "$INSTALL_DIR" --bin-dir "$BIN_DIR" --skip-smoke
test ! -e "$INSTALL_DIR/current" || fail "tampered-sha256: left partial install behind"

expect_fail "invalid-version" "invalid release version" \
  bash "$SCRIPT" --version "../evil" --platform linux-x64 --install-dir "$INSTALL_DIR" --bin-dir "$BIN_DIR"

expect_fail "invalid-repo" "invalid repository name" \
  bash "$SCRIPT" --repo "bad repo/name" --version 0.5.0 --platform linux-x64 --install-dir "$INSTALL_DIR" --bin-dir "$BIN_DIR"

echo "test-install-cli: OK"
