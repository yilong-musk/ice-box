#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Fixture checks for scripts/verify-release-environment.sh.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SCRIPT="$ROOT/scripts/verify-release-environment.sh"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/ice-box-release-env-XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

mkdir -p "$TMP/bin"
cat >"$TMP/bin/gh" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
# FAKE_ENV selects the environment settings:
#   none       no deployment rules     missing  environment does not exist
#   protected  protected branches only tags     custom rules: v* tags only
#   both       custom rules: v* tags and main   error  API failure
case "$*" in
*"/environments/release/deployment-branch-policies "*)
  case "$FAKE_ENV" in
  tags) printf 'tag\tv*\n' ;;
  both) printf 'tag\tv*\nbranch\tmain\n' ;;
  esac
  ;;
*"/environments/release "*)
  case "$FAKE_ENV" in
  none) echo "false false" ;;
  protected) echo "true false" ;;
  tags | both) echo "false true" ;;
  missing)
    echo "gh: Not Found (HTTP 404)" >&2
    exit 1
    ;;
  error)
    echo "gh: Resource not accessible by integration (HTTP 403)" >&2
    exit 1
    ;;
  esac
  ;;
*"/branches/main "*) echo true ;;
*"/branches/feature "*) echo false ;;
*)
  echo "unexpected gh command: $*" >&2
  exit 2
  ;;
esac
STUB
chmod +x "$TMP/bin/gh"

fail() {
  echo "test-verify-release-environment: $*" >&2
  exit 1
}

run() {
  PATH="$TMP/bin:$PATH" FAKE_ENV="$1" GITHUB_REPOSITORY=fixture/repo bash "$SCRIPT" "$2"
}

expect_ok() {
  local out
  out="$(run "$1" "$2" 2>&1)" || fail "$1 $2: expected success: $out"
}

expect_fail() {
  local out
  if out="$(run "$1" "$2" 2>&1)"; then
    fail "$1 $2: expected failure"
  fi
  printf '%s\n' "$out" | grep -Fq "$3" || fail "$1 $2: expected '$3', got: $out"
}

echo "== allowed =="
expect_ok none refs/heads/main
expect_ok none refs/tags/v0.2.8
expect_ok missing refs/heads/main
expect_ok protected refs/heads/main
expect_ok tags refs/tags/v0.2.8
expect_ok both refs/tags/v0.2.8
expect_ok both refs/heads/main

echo "== rejected =="
# The case a tags-only rule set breaks: manual release/rehearse runs on main.
expect_fail tags refs/heads/main "no matching branch rule"
expect_fail both refs/heads/feature "no matching branch rule"
expect_fail protected refs/tags/v0.2.8 "only protected branches"
expect_fail protected refs/heads/feature "only protected branches"
expect_fail error refs/heads/main "HTTP 403"
expect_fail none main "usage:"

echo "test-verify-release-environment: OK"
