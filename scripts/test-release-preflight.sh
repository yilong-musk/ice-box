#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Fixture checks for scripts/release-preflight.sh.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SCRIPT="$ROOT/scripts/release-preflight.sh"
VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$ROOT/Cargo.toml" | head -n 1)"
TAG="v$VERSION"

fail() {
  echo "test-release-preflight: $*" >&2
  exit 1
}

expect_ok() {
  local name="$1"
  shift
  if ! "$SCRIPT" "$@" >/dev/null 2>&1; then
    echo "test-release-preflight: $name: expected success" >&2
    "$SCRIPT" "$@" >&2 || true
    exit 1
  fi
}

expect_fail() {
  local name="$1"
  local needle="$2"
  shift 2
  local err
  if err="$("$SCRIPT" "$@" 2>&1)"; then
    echo "test-release-preflight: $name: expected failure" >&2
    exit 1
  fi
  if ! printf '%s\n' "$err" | grep -q "$needle"; then
    echo "test-release-preflight: $name: missing '$needle' in:" >&2
    printf '%s\n' "$err" >&2
    exit 1
  fi
}

echo "== help =="
if ! "$SCRIPT" --help >/dev/null 2>&1; then
  fail "help-first: expected success"
fi
if ! "$SCRIPT" "$TAG" --help >/dev/null 2>&1; then
  fail "help-after-tag: expected success"
fi

echo "== current version =="
expect_ok "current-tag" "$TAG"
expect_ok "older-latest" "$TAG" --latest-tag v0.0.1
expect_ok "options-before-tag" --latest-tag v0.0.1 "$TAG"
expect_ok "multiple-older-latest" "$TAG" --latest-tag v0.0.1 --latest-tag v0.0.2
expect_ok "existing-release" "$TAG" --latest-tag "$TAG" --allow-existing

echo "== failures =="
expect_fail "tag-without-v" "invalid release tag" "${TAG#v}"
expect_fail "same-version" "must be newer" "$TAG" --latest-tag "$TAG"
expect_fail "older-than-latest" "must be newer" "$TAG" --latest-tag v9.9.9
expect_fail "one-higher-latest" "must be newer" "$TAG" --latest-tag v0.0.1 --latest-tag v9.9.9
expect_fail "version-source-mismatch" "version source mismatch" v9.9.9
expect_fail "android-code-overflow" "below 1000" v1.1000.0
expect_fail "leading-zero-prerelease" "cannot have leading zeros" v0.2.7-rc.01

if "$SCRIPT" >/dev/null 2>&1; then
  fail "missing-tag: expected failure"
fi

echo "== stale generated tauri.properties =="
FIXTURE="$(mktemp -d "${TMPDIR:-/tmp}/ice-box-release-preflight-XXXXXX")"
trap 'rm -rf "$FIXTURE"' EXIT
mkdir -p "$FIXTURE/scripts" \
  "$FIXTURE/apps/desktop/src-tauri" \
  "$FIXTURE/apps/mobile/src-tauri/gen/android/app"
cp "$SCRIPT" "$FIXTURE/scripts/release-preflight.sh"
cp "$ROOT/scripts/release-notes.sh" "$ROOT/scripts/android-version-code.sh" "$FIXTURE/scripts/"
printf 'version = "%s"\n' "$VERSION" >"$FIXTURE/Cargo.toml"
printf '{"version": "%s"}\n' "$VERSION" >"$FIXTURE/apps/desktop/package.json"
printf '{"version": "%s"}\n' "$VERSION" >"$FIXTURE/apps/desktop/src-tauri/tauri.conf.json"
printf '{"version": "%s"}\n' "$VERSION" >"$FIXTURE/apps/mobile/package.json"
CODE="$(bash "$ROOT/scripts/android-version-code.sh" "$VERSION")"
printf '{"version": "%s", "bundle": {"android": {"versionCode": %s}}}\n' "$VERSION" "$CODE" \
  >"$FIXTURE/apps/mobile/src-tauri/tauri.conf.json"
cat >"$FIXTURE/CHANGELOG.md" <<EOF
# Changelog

## [$VERSION] - 2026-01-01

### Changed

- fixture
EOF

# A checkout may contain a tauri.properties left by an older local Android
# build; it is generated and must not fail the preflight.
cat >"$FIXTURE/apps/mobile/src-tauri/gen/android/app/tauri.properties" <<EOF
tauri.android.versionName=0.0.1
tauri.android.versionCode=1
EOF
if ! err="$("$FIXTURE/scripts/release-preflight.sh" "$TAG" 2>&1)"; then
  fail "stale-generated-file: expected success: $err"
fi
if ! printf '%s\n' "$err" | grep -q "ignoring stale generated tauri.properties"; then
  fail "stale-generated-file: warning missing: $err"
fi

echo "test-release-preflight: OK"
