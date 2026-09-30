#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Fixture checks for scripts/android-version-code.sh.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SCRIPT="$ROOT/scripts/android-version-code.sh"

expect() {
  local version="$1"
  local want="$2"
  local got
  got="$("$SCRIPT" "$version")"
  if [[ "$got" != "$want" ]]; then
    echo "android-version-code $version: got $got, want $want" >&2
    exit 1
  fi
}

expect_fail() {
  local version="$1"
  if "$SCRIPT" "$version" >/dev/null 2>&1; then
    echo "android-version-code $version: expected failure" >&2
    exit 1
  fi
}

expect 0.1.15 1015
expect 1.2.3 1002003
expect 0.1.15-rc.1 1015
expect_fail 0.0.0
expect_fail 1.2
expect_fail ""

echo "test-android-version-code: OK"
