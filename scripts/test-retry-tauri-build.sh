#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Fixture tests for scripts/retry-tauri-build.sh.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SCRIPT="$ROOT/scripts/retry-tauri-build.sh"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/ice-box-retry-tauri-test-XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

fail() {
  printf 'test-retry-tauri-build: %b\n' "$*" >&2
  exit 1
}

echo "== retries transient failure on cache miss and clears partial cache =="
COUNTER="$TMP/attempts-1"
CACHE="$TMP/cache-1"
mkdir -p "$CACHE"
printf 'truncated' >"$CACHE/linuxdeploy-plugin-appimage"
CMD="$TMP/flaky.sh"
cat >"$CMD" <<EOF
#!/usr/bin/env bash
set -euo pipefail
n=0
[[ -f "$COUNTER" ]] && n="\$(cat "$COUNTER")"
n=\$((n + 1))
printf '%s' "\$n" >"$COUNTER"
if [[ "\$n" -eq 1 ]]; then
  echo "error sending request for url: connection reset by peer" >&2
  exit 7
fi
if [[ -e "$CACHE/linuxdeploy-plugin-appimage" ]]; then
  echo "stale partial cache was not cleared before retry" >&2
  exit 9
fi
echo "bundle ok"
EOF
chmod +x "$CMD"

out="$(bash "$SCRIPT" --label "AppImage toolset" --cache-hit false --clear-cache-dir "$CACHE" --attempts 3 --delay 0 -- "$CMD" 2>&1)" ||
  fail "transient-retry: expected success, got:\n$out"
[[ "$(cat "$COUNTER")" == "2" ]] || fail "transient-retry: expected 2 attempts, got $(cat "$COUNTER")"
printf '%s\n' "$out" | grep -Fq "retrying in 0s" || fail "transient-retry: missing warning in:\n$out"

echo "== does not retry when cache hit =="
COUNTER="$TMP/attempts-2"
CMD2="$TMP/cache-hit.sh"
cat >"$CMD2" <<EOF
#!/usr/bin/env bash
set -euo pipefail
n=0
[[ -f "$COUNTER" ]] && n="\$(cat "$COUNTER")"
n=\$((n + 1))
printf '%s' "\$n" >"$COUNTER"
echo "HTTP status: 504 Gateway Timeout" >&2
exit 42
EOF
chmod +x "$CMD2"

set +e
bash "$SCRIPT" --label "NSIS toolset" --cache-hit true --attempts 3 --delay 0 -- "$CMD2" >/dev/null 2>&1
rc=$?
set -e
[[ "$rc" -eq 42 ]] || fail "cache-hit-no-retry: expected exit 42, got $rc"
[[ "$(cat "$COUNTER")" == "1" ]] || fail "cache-hit-no-retry: expected 1 attempt, got $(cat "$COUNTER")"

echo "== does not retry non-transient error =="
COUNTER="$TMP/attempts-3"
CMD3="$TMP/compile-err.sh"
cat >"$CMD3" <<EOF
#!/usr/bin/env bash
set -euo pipefail
n=0
[[ -f "$COUNTER" ]] && n="\$(cat "$COUNTER")"
n=\$((n + 1))
printf '%s' "\$n" >"$COUNTER"
echo "error[E0308]: mismatched types" >&2
exit 101
EOF
chmod +x "$CMD3"

set +e
bash "$SCRIPT" --label "AppImage toolset" --cache-hit false --attempts 3 --delay 0 -- "$CMD3" >/dev/null 2>&1
rc=$?
set -e
[[ "$rc" -eq 101 ]] || fail "non-transient: expected exit 101, got $rc"
[[ "$(cat "$COUNTER")" == "1" ]] || fail "non-transient: expected 1 attempt, got $(cat "$COUNTER")"

echo "== exhausted retries preserve original exit status =="
COUNTER="$TMP/attempts-4"
CMD4="$TMP/always-502.sh"
cat >"$CMD4" <<EOF
#!/usr/bin/env bash
set -euo pipefail
n=0
[[ -f "$COUNTER" ]] && n="\$(cat "$COUNTER")"
n=\$((n + 1))
printf '%s' "\$n" >"$COUNTER"
echo "status code: 502 Bad Gateway" >&2
exit 23
EOF
chmod +x "$CMD4"

set +e
bash "$SCRIPT" --label "AppImage toolset" --cache-hit false --attempts 3 --delay 0 -- "$CMD4" >/dev/null 2>&1
rc=$?
set -e
[[ "$rc" -eq 23 ]] || fail "exhausted-status: expected exit 23, got $rc"
[[ "$(cat "$COUNTER")" == "3" ]] || fail "exhausted-status: expected 3 attempts, got $(cat "$COUNTER")"

echo "== rejects missing option value =="
set +e
err="$(bash "$SCRIPT" --label 2>&1)"
rc=$?
set -e
[[ "$rc" -eq 2 ]] || fail "missing-opt-value: expected exit 2, got $rc"
printf '%s\n' "$err" | grep -Fq "requires a value" || fail "missing-opt-value: unexpected error: $err"

echo "test-retry-tauri-build: OK"
