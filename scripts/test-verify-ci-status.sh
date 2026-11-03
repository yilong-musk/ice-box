#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Fixture checks for scripts/verify-ci-status.sh.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SCRIPT="$ROOT/scripts/verify-ci-status.sh"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/ice-box-ci-status-XXXXXX")"
trap 'rm -rf "$TMP"' EXIT
SHA="0123456789abcdef0123456789abcdef01234567"

mkdir -p "$TMP/bin"
cat >"$TMP/bin/gh" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" != api || "$*" != *"/commits/0123456789abcdef0123456789abcdef01234567/check-runs"* ]]; then
  echo "unexpected gh command: $*" >&2
  exit 2
fi
row() { printf '%s\t%s\t%s\t%s\n' "$@"; }
green() {
  row 10 "gate (linux)" completed success
  row 11 "gate + build (macOS dmg)" completed success
  row 12 "gate + build (Windows nsis)" completed success
  row 13 "test (macOS)" completed success
}
case "${FAKE_CI:-}" in
green) green ;;
rerun-green)
  row 5 "gate (linux)" completed failure
  green
  ;;
rerun-red)
  green
  row 20 "gate (linux)" completed failure
  ;;
pending)
  row 10 "gate (linux)" in_progress ""
  row 11 "gate + build (macOS dmg)" completed success
  row 12 "gate + build (Windows nsis)" completed success
  ;;
cancelled)
  row 10 "gate (linux)" completed cancelled
  row 11 "gate + build (macOS dmg)" completed success
  row 12 "gate + build (Windows nsis)" completed success
  ;;
missing)
  row 10 "gate (linux)" completed success
  ;;
error)
  echo "gh: Resource not accessible by integration (HTTP 403)" >&2
  exit 1
  ;;
*)
  echo "unexpected FAKE_CI" >&2
  exit 2
  ;;
esac
STUB
chmod +x "$TMP/bin/gh"

fail() {
  echo "test-verify-ci-status: $*" >&2
  exit 1
}

run() {
  PATH="$TMP/bin:$PATH" FAKE_CI="$1" GITHUB_REPOSITORY=fixture/repo bash "$SCRIPT" "$SHA"
}

expect_ok() {
  local out
  out="$(run "$1" 2>&1)" || fail "$1: expected success: $out"
}

expect_fail() {
  local out
  if out="$(run "$1" 2>&1)"; then
    fail "$1: expected failure"
  fi
  printf '%s\n' "$out" | grep -Fq "$2" || fail "$1: expected '$2', got: $out"
}

echo "== passing =="
expect_ok green
expect_ok rerun-green

echo "== failures =="
expect_fail rerun-red "concluded 'failure'"
expect_fail pending "still in_progress"
expect_fail cancelled "concluded 'cancelled'"
expect_fail missing "has not run"
expect_fail error "HTTP 403"
if GITHUB_REPOSITORY=fixture/repo bash "$SCRIPT" not-a-sha >/dev/null 2>&1; then
  fail "bad-sha: expected failure"
fi

echo "test-verify-ci-status: OK"
