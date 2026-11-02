#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Fixture checks for scripts/gh-release-state.sh.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SCRIPT="$ROOT/scripts/gh-release-state.sh"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/ice-box-release-state-XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

mkdir -p "$TMP/bin"
cat >"$TMP/bin/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" != "api" || "${2:-}" != "graphql" ]]; then
  echo "unexpected gh command: $*" >&2
  exit 2
fi
case "${FAKE_STATE:-}" in
draft)
  printf '{"data":{"repository":{"release":{"databaseId":1,"isDraft":true,"isPrerelease":false}}}}\n'
  ;;
published)
  printf '{"data":{"repository":{"release":{"databaseId":1,"isDraft":false,"isPrerelease":false}}}}\n'
  ;;
prerelease)
  printf '{"data":{"repository":{"release":{"databaseId":1,"isDraft":false,"isPrerelease":true}}}}\n'
  ;;
missing)
  printf '{"data":{"repository":{"release":null}}}\n'
  ;;
error)
  echo "gh: Bad credentials (HTTP 401)" >&2
  exit 1
  ;;
*)
  echo "unexpected FAKE_STATE" >&2
  exit 2
  ;;
esac
EOF
chmod +x "$TMP/bin/gh"

run() {
  local state="$1"
  shift
  PATH="$TMP/bin:$PATH" FAKE_STATE="$state" GITHUB_REPOSITORY=fixture/repo \
    bash "$SCRIPT" "$@"
}

expect_state() {
  local name="$1"
  local state="$2"
  local want="$3"
  local got
  if ! got="$(run "$state" v0.2.6)"; then
    echo "test-gh-release-state: $name: expected success" >&2
    exit 1
  fi
  if [[ "$got" != "$want" ]]; then
    echo "test-gh-release-state: $name: got '$got', want '$want'" >&2
    exit 1
  fi
}

expect_kind() {
  local name="$1"
  local state="$2"
  local want="$3"
  local got
  if ! got="$(run "$state" --kind v0.2.6)"; then
    echo "test-gh-release-state: $name: expected success" >&2
    exit 1
  fi
  if [[ "$got" != "$want" ]]; then
    echo "test-gh-release-state: $name: got '$got', want '$want'" >&2
    exit 1
  fi
}

expect_fail() {
  local name="$1"
  local state="$2"
  local needle="$3"
  local err
  if err="$(run "$state" v0.2.6 2>&1)"; then
    echo "test-gh-release-state: $name: expected failure" >&2
    exit 1
  fi
  if ! printf '%s\n' "$err" | grep -q -- "$needle"; then
    echo "test-gh-release-state: $name: missing '$needle' in:" >&2
    printf '%s\n' "$err" >&2
    exit 1
  fi
}

echo "== states =="
expect_state "draft" draft "true"
expect_state "published" published "false"
expect_state "missing" missing "missing"

echo "== kinds =="
expect_kind "draft" draft "draft"
expect_kind "published" published "final"
expect_kind "prerelease" prerelease "prerelease"
expect_kind "missing" missing "missing"

echo "== failures =="
expect_fail "api-error" error "Bad credentials"

echo "test-gh-release-state: OK"
