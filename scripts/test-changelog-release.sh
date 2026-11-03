#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Fixture checks for scripts/changelog-release.sh.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SCRIPT="$ROOT/scripts/changelog-release.sh"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/ice-box-changelog-release-XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

fail() {
  echo "test-changelog-release: $*" >&2
  exit 1
}

write_changelog() {
  cat >"$TMP/CHANGELOG.md"
}

expect_fail() {
  local name="$1"
  local needle="$2"
  shift 2
  local err
  if err="$(bash "$SCRIPT" "$@" --file "$TMP/CHANGELOG.md" 2>&1)"; then
    fail "$name: expected failure"
  fi
  if ! printf '%s\n' "$err" | grep -Fq "$needle"; then
    fail "$name: expected '$needle', got: $err"
  fi
}

echo "== happy path =="
write_changelog <<'EOF'
# Changelog

## [Unreleased]

### Fixed

- a fix

## [0.2.7] - 2026-11-02

### Added

- older entry
EOF
bash "$SCRIPT" 0.2.8 --date 2026-11-05 --file "$TMP/CHANGELOG.md" >/dev/null
expected="$(
  cat <<'EOF'
# Changelog

## [Unreleased]

## [0.2.8] - 2026-11-05

### Fixed

- a fix

## [0.2.7] - 2026-11-02

### Added

- older entry
EOF
)"
[[ "$(cat "$TMP/CHANGELOG.md")" == "$expected" ]] || fail "happy path: unexpected output:
$(cat "$TMP/CHANGELOG.md")"

# The rolled file must satisfy the release-notes lookup the preflight uses.
grep -q '^## \[0.2.8\] - 2026-11-05$' "$TMP/CHANGELOG.md" || fail "heading missing"

echo "== failures =="
# Rolling again finds the new, empty Unreleased section and the existing version.
expect_fail "existing-version" "already has a section" 0.2.8 --date 2026-11-05
expect_fail "empty-unreleased" "is empty" 0.2.9 --date 2026-11-05

write_changelog <<'EOF'
# Changelog

## [0.2.7] - 2026-11-02
EOF
expect_fail "no-unreleased" "exactly one" 0.2.8

write_changelog <<'EOF'
# Changelog

## [Unreleased]

- loose bullet without a subsection
EOF
expect_fail "no-subsection" "no '### " 0.2.8

expect_fail "bad-date" "invalid date" 0.2.8 --date 11/05/2026
if bash "$SCRIPT" not-a-version --file "$TMP/CHANGELOG.md" >/dev/null 2>&1; then
  fail "bad-version: expected failure"
fi

echo "test-changelog-release: OK"
