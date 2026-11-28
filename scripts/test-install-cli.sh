#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Offline fixture tests for scripts/install-cli.sh and scripts/install-cli.ps1.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SCRIPT="$ROOT/scripts/install-cli.sh"
PS_SCRIPT="$ROOT/scripts/install-cli.ps1"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/ice-box-test-install-cli-XXXXXX")"
HTTP_PID=""
cleanup() {
  if [[ -n "${HTTP_PID:-}" ]]; then
    kill "$HTTP_PID" 2>/dev/null || true
    wait "$HTTP_PID" 2>/dev/null || true
  fi
  rm -rf "$TMP"
}
trap cleanup EXIT

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
  local exit_code="${5:-0}"
  local stem="ice-box-cli_${ver}_${target}"
  local archive="${stem}.tar.gz"
  local work="$TMP/stage-${ver}-${RANDOM}"
  local stage="$work/$stem"

  mkdir -p "$dir" "$stage/resources/geoip"
  cat >"$stage/ice-box-cli" <<EOF
#!/bin/sh
echo "ice-box-cli ${ver} (${marker})"
exit ${exit_code}
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
  ICE_BOX_PS_SCRIPT="$PS_SCRIPT" pwsh -NoProfile -NonInteractive -Command '
    $ErrorActionPreference = "Stop"
    $errors = $null
    [void][System.Management.Automation.Language.Parser]::ParseFile($env:ICE_BOX_PS_SCRIPT, [ref]$null, [ref]$errors)
    if ($errors -and $errors.Count -gt 0) {
      $errors | ForEach-Object { [Console]::Error.WriteLine($_.ToString()) }
      exit 1
    }
  '
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

echo "== smoke failure leaves the previous install in place =="
SMOKE_INSTALL="$TMP/opt-smoke"
SMOKE_BIN="$TMP/bin-smoke"
REL_KEEP="$TMP/rel-keep"
make_fixture_release "$REL_KEEP" "0.5.0" "linux_x86_64" "keep"
ICE_BOX_RELEASE_BASE_URL="file://$REL_KEEP" \
  bash "$SCRIPT" \
  --version 0.5.0 \
  --platform linux-x64 \
  --install-dir "$SMOKE_INSTALL" \
  --bin-dir "$SMOKE_BIN" >/dev/null
REL_BAD="$TMP/rel-bad-smoke"
make_fixture_release "$REL_BAD" "0.5.9" "linux_x86_64" "bad-smoke" 1
expect_fail "smoke-rollback" "staged binary failed smoke check" \
  env ICE_BOX_RELEASE_BASE_URL="file://$REL_BAD" \
  bash "$SCRIPT" --version 0.5.9 --platform linux-x64 --install-dir "$SMOKE_INSTALL" --bin-dir "$SMOKE_BIN"
"$SMOKE_INSTALL/current/ice-box-cli" | grep -Fq "ice-box-cli 0.5.0 (keep)" ||
  fail "smoke-rollback: previous install was replaced"
if find "$SMOKE_INSTALL" -name '.staging-*' -print | grep -q .; then
  fail "smoke-rollback: staging directory left behind"
fi
"$SMOKE_BIN/ice-box-cli" | grep -Fq "ice-box-cli 0.5.0 (keep)" ||
  fail "smoke-rollback: symlink no longer runs the previous install"

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

expect_fail "mac-x64-unpublished" "Apple Silicon" \
  bash "$SCRIPT" --version 0.5.0 --platform mac-x64 --install-dir "$INSTALL_DIR" --bin-dir "$BIN_DIR"

expect_fail "mac-x64-custom-base" "failed to download" \
  env ICE_BOX_RELEASE_BASE_URL="file://$TMP/no-such-release" \
  bash "$SCRIPT" --version 0.5.0 --platform mac-x64 --install-dir "$INSTALL_DIR" --bin-dir "$BIN_DIR"

make_fixture_zip() {
  local dir="$1"
  local ver="$2"
  local marker="${3:-ok}"
  local exit_code="${4:-0}"
  local stem="ice-box-cli_${ver}_windows_x86_64"
  local archive="${stem}.zip"
  local work="$TMP/zipstage-${ver}-${RANDOM}"
  local stage="$work/$stem"

  mkdir -p "$dir" "$stage/resources"
  cat >"$stage/ice-box-cli.exe" <<EOF
#!/bin/sh
echo "ice-box-cli ${ver} (${marker})"
exit ${exit_code}
EOF
  printf 'sb\n' >"$stage/resources/sing-box.exe"
  chmod +x "$stage/ice-box-cli.exe"

  python3 - "$work" "$dir/$archive" "$stem" <<'PY'
import os
import sys
import zipfile

work, dest, stem = sys.argv[1:]
with zipfile.ZipFile(dest, "w", compression=zipfile.ZIP_DEFLATED) as archive:
    root = os.path.join(work, stem)
    for dirpath, _, names in os.walk(root):
        for name in names:
            full = os.path.join(dirpath, name)
            rel = os.path.relpath(full, work).replace(os.sep, "/")
            info = zipfile.ZipInfo(rel)
            info.external_attr = (0o755 if name.endswith(".exe") else 0o644) << 16
            with open(full, "rb") as handle:
                archive.writestr(info, handle.read())
PY
  local sum
  sum="$(sha256_file "$dir/$archive")"
  if [[ -f "$dir/SHA256SUMS" ]]; then
    grep -Fv -- "  ${archive}" "$dir/SHA256SUMS" >"$dir/SHA256SUMS.tmp" || true
    mv "$dir/SHA256SUMS.tmp" "$dir/SHA256SUMS"
  fi
  printf '%s  %s\n' "$sum" "$archive" >>"$dir/SHA256SUMS"
}

case "$(uname -s)" in
MINGW* | MSYS* | CYGWIN*) ps_unix=0 ;;
*) ps_unix=1 ;;
esac

if command -v pwsh >/dev/null 2>&1 && [[ "$ps_unix" == "1" ]]; then
  echo "== pwsh install + upgrade + smoke rollback + uninstall =="
  PS_REL="$TMP/ps-rel"
  make_fixture_zip "$PS_REL" "0.5.0" "v1" 0
  make_fixture_zip "$PS_REL" "0.5.1" "v2" 0
  make_fixture_zip "$PS_REL" "0.5.9" "bad" 1
  PS_PORT="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')"
  python3 -m http.server "$PS_PORT" --bind 127.0.0.1 --directory "$PS_REL" >"$TMP/ps-http.log" 2>&1 &
  HTTP_PID=$!
  ready=0
  for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
    if curl -fsS -o /dev/null "http://127.0.0.1:${PS_PORT}/SHA256SUMS" 2>/dev/null; then
      ready=1
      break
    fi
    sleep 0.1
  done
  [[ "$ready" == "1" ]] || fail "ps-http: fixture server did not start ($(cat "$TMP/ps-http.log"))"

  PS_INSTALL="$TMP/ps-opt"
  ICE_BOX_RELEASE_BASE_URL="http://127.0.0.1:${PS_PORT}" \
    ICE_BOX_SKIP_ARCH_CHECK=1 \
    pwsh -NoProfile -NonInteractive -File "$PS_SCRIPT" \
    -Version 0.5.0 \
    -InstallDir "$PS_INSTALL" \
    -NoPathUpdate
  "$PS_INSTALL/current/ice-box-cli.exe" | grep -Fq "ice-box-cli 0.5.0 (v1)" ||
    fail "ps-install: expected v1 smoke output"

  touch "$PS_INSTALL/current/stale-marker"
  ICE_BOX_RELEASE_BASE_URL="http://127.0.0.1:${PS_PORT}" \
    ICE_BOX_SKIP_ARCH_CHECK=1 \
    pwsh -NoProfile -NonInteractive -File "$PS_SCRIPT" \
    -Version 0.5.1 \
    -InstallDir "$PS_INSTALL" \
    -NoPathUpdate
  "$PS_INSTALL/current/ice-box-cli.exe" | grep -Fq "ice-box-cli 0.5.1 (v2)" ||
    fail "ps-upgrade: expected v2 smoke output"
  test ! -e "$PS_INSTALL/current/stale-marker" || fail "ps-upgrade: stale file survived upgrade"

  expect_fail "ps-smoke-rollback" "staged binary failed smoke check" \
    env ICE_BOX_RELEASE_BASE_URL="http://127.0.0.1:${PS_PORT}" \
    ICE_BOX_SKIP_ARCH_CHECK=1 \
    pwsh -NoProfile -NonInteractive -File "$PS_SCRIPT" \
    -Version 0.5.9 \
    -InstallDir "$PS_INSTALL" \
    -NoPathUpdate
  "$PS_INSTALL/current/ice-box-cli.exe" | grep -Fq "ice-box-cli 0.5.1 (v2)" ||
    fail "ps-smoke-rollback: previous install was replaced"
  if find "$PS_INSTALL" -name '.staging-*' -print | grep -q .; then
    fail "ps-smoke-rollback: staging directory left behind"
  fi

  ICE_BOX_SKIP_ARCH_CHECK=1 \
    pwsh -NoProfile -NonInteractive -File "$PS_SCRIPT" \
    -Uninstall \
    -InstallDir "$PS_INSTALL"
  test ! -e "$PS_INSTALL" || fail "ps-uninstall: install dir not removed"
fi

echo "test-install-cli: OK"
