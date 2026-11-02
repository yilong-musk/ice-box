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

echo "== current version =="
expect_ok "current-tag" "$TAG"
expect_ok "older-latest" "$TAG" --latest-tag v0.0.1
expect_ok "multiple-older-latest" "$TAG" --latest-tag v0.0.1 --latest-tag v0.0.2
expect_ok "existing-release" "$TAG" --latest-tag "$TAG" --allow-existing

echo "== failures =="
expect_fail "tag-without-v" "invalid release tag" "${TAG#v}"
expect_fail "same-version" "must be newer" "$TAG" --latest-tag "$TAG"
expect_fail "older-than-latest" "must be newer" "$TAG" --latest-tag v9.9.9
expect_fail "one-higher-latest" "must be newer" "$TAG" --latest-tag v0.0.1 --latest-tag v9.9.9
expect_fail "version-source-mismatch" "version source mismatch" v9.9.9
expect_fail "android-code-overflow" "below 1000" v1.1000.0

if "$SCRIPT" >/dev/null 2>&1; then
  fail "missing-tag: expected failure"
fi

echo "test-release-preflight: OK"
