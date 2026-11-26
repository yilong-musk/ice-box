#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Fixture tests for scripts/verify-linux-bundles.sh.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SCRIPT="$ROOT/scripts/verify-linux-bundles.sh"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/ice-box-verify-linux-test-XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

fail() {
  printf 'test-verify-linux-bundles: %b\n' "$*" >&2
  exit 1
}

# Build a stub `dpkg-deb` that treats a `.deb` fixture as a tar archive of its
# payload directory and a stub glibc script that records every inspected path.
FAKEBIN="$TMP/bin"
mkdir -p "$FAKEBIN"

cat >"$FAKEBIN/dpkg-deb" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
mode="${1:-}"
archive="${2:-}"
case "$mode" in
-c)
  tar -tzf "$archive" | while IFS= read -r entry; do
    case "$entry" in
    */) continue ;;
    esac
    printf -- '-rwxr-xr-x root/root 10 2026-01-01 00:00 ./%s\n' "${entry#./}"
  done
  ;;
-x)
  dest="${3:?}"
  mkdir -p "$dest"
  tar -xzf "$archive" -C "$dest"
  ;;
*)
  echo "unexpected dpkg-deb args: $*" >&2
  exit 2
  ;;
esac
EOF
chmod +x "$FAKEBIN/dpkg-deb"

GLIBC_LOG="$TMP/glibc.log"
GLIBC_STUB="$TMP/glibc-stub.sh"
cat >"$GLIBC_STUB" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
max="$1"
shift
for path in "$@"; do
  printf '%s %s\n' "$max" "$path" >>"${GLIBC_LOG:?}"
done
if [[ "${GLIBC_FAIL_ON:-}" != "" ]]; then
  for path in "$@"; do
    if [[ "$path" == *"$GLIBC_FAIL_ON"* ]]; then
      echo "check-glibc-floor: FAIL $path: synthetic glibc failure" >&2
      exit 1
    fi
  done
fi
EOF
chmod +x "$GLIBC_STUB"

populate_tree() {
  local root="$1"
  mkdir -p "$root/usr/bin" "$root/usr/lib/ice-box/geoip"
  printf '#!/bin/sh\n' >"$root/usr/bin/ice-box"
  printf '#!/bin/sh\n' >"$root/usr/lib/ice-box/sing-box"
  printf '#!/bin/sh\n' >"$root/usr/lib/ice-box/ice-helper"
  chmod +x \
    "$root/usr/bin/ice-box" \
    "$root/usr/lib/ice-box/sing-box" \
    "$root/usr/lib/ice-box/ice-helper"
  printf 'geoip\n' >"$root/usr/lib/ice-box/geoip/geoip-cn.srs"
  printf 'geosite\n' >"$root/usr/lib/ice-box/geoip/geosite-cn.srs"
}

# make_fixture <dir> <ver> <deb_arch> <appimage_arch> [options...]
# Options via env:
#   OMIT_DEB_FILE, EMPTY_DEB_FILE, OMIT_APPIMAGE_FILE, EMPTY_APPIMAGE_FILE,
#   OMIT_APPRUN, WITH_SIG
make_fixture() {
  local dir="$1"
  local ver="$2"
  local deb_arch="$3"
  local appimage_arch="$4"
  local deb_src="$dir/deb-src"
  local app_src="$dir/app-src"

  mkdir -p "$dir/bundle/deb" "$dir/bundle/appimage" "$deb_src" "$app_src"
  populate_tree "$deb_src"
  populate_tree "$app_src"
  if [[ "${OMIT_APPRUN:-0}" != "1" ]]; then
    printf '#!/bin/sh\n' >"$app_src/AppRun"
    chmod +x "$app_src/AppRun"
  fi
  if [[ -n "${OMIT_DEB_FILE:-}" ]]; then
    rm -f "$deb_src/$OMIT_DEB_FILE"
  fi
  if [[ -n "${EMPTY_DEB_FILE:-}" ]]; then
    : >"$deb_src/$EMPTY_DEB_FILE"
  fi
  if [[ -n "${OMIT_APPIMAGE_FILE:-}" ]]; then
    rm -f "$app_src/$OMIT_APPIMAGE_FILE"
  fi
  if [[ -n "${EMPTY_APPIMAGE_FILE:-}" ]]; then
    : >"$app_src/$EMPTY_APPIMAGE_FILE"
  fi

  (cd "$deb_src" && tar -czf "$dir/bundle/deb/ice-box_${ver}_${deb_arch}.deb" .)

  local appimage="$dir/bundle/appimage/ice-box_${ver}_${appimage_arch}.AppImage"
  cat >"$appimage" <<EOF
#!/usr/bin/env bash
set -euo pipefail
if [[ "\${1:-}" != "--appimage-extract" ]]; then
  echo "expected --appimage-extract, got: \$*" >&2
  exit 2
fi
mkdir -p squashfs-root
cp -R "$app_src/." squashfs-root/
EOF
  chmod +x "$appimage"

  if [[ "${WITH_SIG:-0}" == "1" ]]; then
    printf 'sig-bytes\n' >"${appimage}.sig"
  fi
}

run_verify() {
  PATH="$FAKEBIN:$PATH" GLIBC_LOG="$GLIBC_LOG" bash "$SCRIPT" \
    --glibc-script "$GLIBC_STUB" "$@"
}

expect_ok() {
  local name="$1"
  shift
  : >"$GLIBC_LOG"
  if ! run_verify "$@" >/dev/null 2>&1; then
    echo "test-verify-linux-bundles: $name: expected success" >&2
    run_verify "$@" >&2 || true
    exit 1
  fi
}

expect_fail() {
  local name="$1"
  local needle="$2"
  shift 2
  : >"$GLIBC_LOG"
  local err
  if err="$(run_verify "$@" 2>&1)"; then
    fail "$name: expected failure"
  fi
  if ! printf '%s\n' "$err" | grep -Fq -- "$needle"; then
    fail "$name: missing '$needle' in:\n$err"
  fi
}

echo "== happy path (x64, .deb + .AppImage + .sig) =="
WITH_SIG=1 make_fixture "$TMP/x64" 0.4.1 amd64 amd64
expect_ok "x64-release" \
  --bundle-dir "$TMP/x64/bundle" \
  --version 0.4.1 \
  --deb-arch amd64 \
  --appimage-arch amd64 \
  --require-sig
# Both deb (3 binaries) and AppImage (3 binaries) must be glibc-checked.
if [[ "$(wc -l <"$GLIBC_LOG" | tr -d ' ')" != "6" ]]; then
  fail "x64-release: expected 6 glibc checks (deb + appimage), got:\n$(cat "$GLIBC_LOG")"
fi
grep -Fq '/deb/usr/bin/ice-box' "$GLIBC_LOG" || fail "x64-release: missing deb ice-box glibc check"
grep -Fq '/appimage/squashfs-root/usr/bin/ice-box' "$GLIBC_LOG" || fail "x64-release: missing appimage ice-box glibc check"

echo "== aarch64 arm64.AppImage normalization =="
WITH_SIG=1 make_fixture "$TMP/arm64" 0.4.1 arm64 arm64
expect_ok "arm64-normalize" \
  --bundle-dir "$TMP/arm64/bundle" \
  --version 0.4.1 \
  --deb-arch arm64 \
  --appimage-arch aarch64 \
  --require-sig
test -s "$TMP/arm64/bundle/appimage/ice-box_0.4.1_aarch64.AppImage" || fail "arm64-normalize: AppImage not renamed"
test -s "$TMP/arm64/bundle/appimage/ice-box_0.4.1_aarch64.AppImage.sig" || fail "arm64-normalize: sig not renamed"

echo "== failures =="
make_fixture "$TMP/bad-deb-name" 0.4.1 x64 amd64
expect_fail "bad-deb-name" "unexpected .deb artifact name" \
  --bundle-dir "$TMP/bad-deb-name/bundle" \
  --version 0.4.1 \
  --deb-arch amd64 \
  --appimage-arch amd64

make_fixture "$TMP/bad-appimage-name" 0.4.1 amd64 x86_64
expect_fail "bad-appimage-name" "unexpected AppImage artifact name" \
  --bundle-dir "$TMP/bad-appimage-name/bundle" \
  --version 0.4.1 \
  --deb-arch amd64 \
  --appimage-arch amd64

OMIT_DEB_FILE="usr/lib/ice-box/geoip/geosite-cn.srs" make_fixture "$TMP/missing-deb-geosite" 0.4.1 amd64 amd64
expect_fail "missing-deb-geosite" "missing /usr/lib/ice-box/geoip/geosite-cn.srs in .deb" \
  --bundle-dir "$TMP/missing-deb-geosite/bundle" \
  --version 0.4.1 \
  --deb-arch amd64 \
  --appimage-arch amd64

OMIT_APPIMAGE_FILE="usr/lib/ice-box/ice-helper" make_fixture "$TMP/missing-appimage-helper" 0.4.1 amd64 amd64
expect_fail "missing-appimage-helper" "missing or empty /usr/lib/ice-box/ice-helper in AppImage" \
  --bundle-dir "$TMP/missing-appimage-helper/bundle" \
  --version 0.4.1 \
  --deb-arch amd64 \
  --appimage-arch amd64

OMIT_APPIMAGE_FILE="usr/lib/ice-box/geoip/geosite-cn.srs" make_fixture "$TMP/missing-appimage-geosite" 0.4.1 amd64 amd64
expect_fail "missing-appimage-geosite" "missing or empty /usr/lib/ice-box/geoip/geosite-cn.srs in AppImage" \
  --bundle-dir "$TMP/missing-appimage-geosite/bundle" \
  --version 0.4.1 \
  --deb-arch amd64 \
  --appimage-arch amd64

EMPTY_DEB_FILE="usr/lib/ice-box/geoip/geosite-cn.srs" make_fixture "$TMP/empty-deb-geosite" 0.4.1 amd64 amd64
expect_fail "empty-deb-geosite" "missing or empty /usr/lib/ice-box/geoip/geosite-cn.srs in extracted .deb" \
  --bundle-dir "$TMP/empty-deb-geosite/bundle" \
  --version 0.4.1 \
  --deb-arch amd64 \
  --appimage-arch amd64

EMPTY_APPIMAGE_FILE="usr/lib/ice-box/geoip/geosite-cn.srs" make_fixture "$TMP/empty-appimage-geosite" 0.4.1 amd64 amd64
expect_fail "empty-appimage-geosite" "missing or empty /usr/lib/ice-box/geoip/geosite-cn.srs in AppImage" \
  --bundle-dir "$TMP/empty-appimage-geosite/bundle" \
  --version 0.4.1 \
  --deb-arch amd64 \
  --appimage-arch amd64

set +e
err="$(run_verify --version 2>&1)"
rc=$?
set -e
[[ "$rc" -eq 2 ]] || fail "missing-opt-value: expected exit 2, got $rc"
printf '%s\n' "$err" | grep -Fq "requires a value" || fail "missing-opt-value: unexpected error: $err"

OMIT_APPRUN=1 make_fixture "$TMP/missing-apprun" 0.4.1 amd64 amd64
expect_fail "missing-apprun" "missing executable AppRun" \
  --bundle-dir "$TMP/missing-apprun/bundle" \
  --version 0.4.1 \
  --deb-arch amd64 \
  --appimage-arch amd64

make_fixture "$TMP/missing-sig" 0.4.1 amd64 amd64
expect_fail "missing-sig" "expected exactly 1 .AppImage.sig" \
  --bundle-dir "$TMP/missing-sig/bundle" \
  --version 0.4.1 \
  --deb-arch amd64 \
  --appimage-arch amd64 \
  --require-sig

make_fixture "$TMP/appimage-glibc-fail" 0.4.1 amd64 amd64
GLIBC_FAIL_ON="/appimage/squashfs-root/usr/bin/ice-box" expect_fail "appimage-glibc-fail" "synthetic glibc failure" \
  --bundle-dir "$TMP/appimage-glibc-fail/bundle" \
  --version 0.4.1 \
  --deb-arch amd64 \
  --appimage-arch amd64

echo "test-verify-linux-bundles: OK"
