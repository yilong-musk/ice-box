#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Fixture checks for scripts/check-doc-versions.sh.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SCRIPT="$ROOT/scripts/check-doc-versions.sh"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/ice-box-doc-versions-XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

fail() {
  echo "test-check-doc-versions: $*" >&2
  exit 1
}

badge() {
  printf '[![Core](https://img.shields.io/badge/sing--box-%s-6f42c1)](https://example.invalid)\n' "$1"
}

# write_repo <pinned> <readme> <readme.zh-CN> <tun>: a fixture repo whose
# three documents name the given versions.
write_repo() {
  rm -rf "$TMP/repo"
  mkdir -p "$TMP/repo/third_party/sing-box" "$TMP/repo/docs"
  printf '%s\n' "$1" >"$TMP/repo/third_party/sing-box/VERSION"
  badge "$2" >"$TMP/repo/README.md"
  badge "$3" >"$TMP/repo/README.zh-CN.md"
  printf 'The desktop TUN path, with the bundled\nsing-box **%s**.\n' "$4" >"$TMP/repo/docs/tun.md"
}

expect_ok() {
  local out
  out="$(bash "$SCRIPT" "$TMP/repo" 2>&1)" || fail "$1: expected success: $out"
}

expect_fail() {
  local out
  if out="$(bash "$SCRIPT" "$TMP/repo" 2>&1)"; then
    fail "$1: expected failure"
  fi
  printf '%s\n' "$out" | grep -Fq "$2" || fail "$1: expected '$2', got: $out"
}

echo "== passing =="
write_repo 1.14.4 1.14.4 1.14.4 1.14.4
expect_ok all-match
bash "$SCRIPT" "$TMP/repo" | grep -Fq "matches sing-box 1.14.4" || fail "all-match: expected the version"

# A major.minor mention in prose is not a pinned version and is not checked.
printf 'sing-box 1.14 defaults to hijack.\n' >>"$TMP/repo/docs/tun.md"
expect_ok prose-mention

echo "== failures =="
write_repo 1.15.0 1.14.4 1.15.0 1.15.0
expect_fail stale-readme "README.md: sing-box badge says 1.14.4, pinned core is 1.15.0"

write_repo 1.15.0 1.15.0 1.14.4 1.15.0
expect_fail stale-readme-zh "README.zh-CN.md: sing-box badge says 1.14.4, pinned core is 1.15.0"

write_repo 1.15.0 1.15.0 1.15.0 1.14.4
expect_fail stale-tun "docs/tun.md: sing-box version says 1.14.4, pinned core is 1.15.0"

# Every occurrence counts: a second, stale mention fails although the first matches.
write_repo 1.15.0 1.15.0 1.15.0 1.15.0
badge 1.14.4 >>"$TMP/repo/README.md"
expect_fail second-occurrence "README.md: sing-box badge says 1.14.4, pinned core is 1.15.0"

write_repo 1.15.0 1.15.0 1.15.0 1.15.0
printf 'no badge here\n' >"$TMP/repo/README.md"
expect_fail missing-badge "README.md: no sing-box badge found"

write_repo 1.15.0 1.15.0 1.15.0 1.15.0
rm "$TMP/repo/docs/tun.md"
expect_fail missing-file "docs/tun.md: file is missing"

write_repo "" 1.15.0 1.15.0 1.15.0
expect_fail empty-version "third_party/sing-box/VERSION is empty"

echo "test-check-doc-versions: OK"
