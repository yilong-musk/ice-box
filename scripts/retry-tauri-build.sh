#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Retry a Tauri bundle build when a cache miss hits a transient toolset
# download error (HTTP 5xx or network failure). Compile or configuration errors
# fail immediately on the first attempt.
#
# Usage:
#   scripts/retry-tauri-build.sh --label <name> \
#     [--cache-hit <true|false>] [--clear-cache-dir <path>] \
#     [--attempts <n>] [--delay <secs>] -- <command...>
set -euo pipefail

LABEL="Tauri toolset"
CACHE_HIT="false"
CLEAR_CACHE_DIR=""
ATTEMPTS=3
DELAY=30

usage() {
  cat >&2 <<'EOF'
usage: scripts/retry-tauri-build.sh --label <name> \
  [--cache-hit <true|false>] [--clear-cache-dir <path>] \
  [--attempts <n>] [--delay <secs>] -- <command...>
EOF
}

require_value() {
  local flag="$1" argc="$2" value="${3:-}"
  if ((argc < 2)) || [[ -z "$value" ]]; then
    echo "retry-tauri-build: $flag requires a value" >&2
    usage
    exit 2
  fi
}

while (($# > 0)); do
  case "$1" in
  --label)
    require_value "$1" "$#" "${2:-}"
    LABEL="$2"
    shift 2
    ;;
  --cache-hit)
    require_value "$1" "$#" "${2:-}"
    CACHE_HIT="$2"
    shift 2
    ;;
  --clear-cache-dir)
    require_value "$1" "$#" "${2:-}"
    CLEAR_CACHE_DIR="$2"
    shift 2
    ;;
  --attempts)
    require_value "$1" "$#" "${2:-}"
    ATTEMPTS="$2"
    shift 2
    ;;
  --delay)
    require_value "$1" "$#" "${2:-}"
    DELAY="$2"
    shift 2
    ;;
  -h | --help)
    usage
    exit 0
    ;;
  --)
    shift
    break
    ;;
  *)
    echo "retry-tauri-build: unknown argument: $1" >&2
    usage
    exit 2
    ;;
  esac
done

if (($# == 0)); then
  echo "retry-tauri-build: missing command after --" >&2
  usage
  exit 2
fi

if ! [[ "$ATTEMPTS" =~ ^[1-9][0-9]*$ ]]; then
  echo "retry-tauri-build: --attempts must be a positive integer (got '$ATTEMPTS')" >&2
  exit 2
fi
if ! [[ "$DELAY" =~ ^[0-9]+$ ]]; then
  echo "retry-tauri-build: --delay must be a non-negative integer (got '$DELAY')" >&2
  exit 2
fi

log="$(mktemp "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/tauri-build-XXXXXX.log")"
trap 'rm -f "$log"' EXIT

TRANSIENT_RE='status code:? 5[0-9]{2}|http status:? 5[0-9]{2}|error sending request|connection (reset|refused|closed)|operation timed out'

status=1
for ((attempt = 1; attempt <= ATTEMPTS; attempt++)); do
  set +e
  "$@" 2>&1 | tee "$log"
  status=${PIPESTATUS[0]}
  set -e
  if [[ "$status" -eq 0 ]]; then
    exit 0
  fi
  if [[ "$CACHE_HIT" == "true" ]] || ! grep -qiE "$TRANSIENT_RE" "$log"; then
    exit "$status"
  fi
  if ((attempt < ATTEMPTS)); then
    if [[ -n "$CLEAR_CACHE_DIR" && "$CLEAR_CACHE_DIR" != "/" && "$CLEAR_CACHE_DIR" != "${HOME:-}" ]]; then
      rm -rf "$CLEAR_CACHE_DIR"
    fi
    echo "::warning::${LABEL} download failed (attempt ${attempt}/${ATTEMPTS}); retrying in ${DELAY}s"
    if ((DELAY > 0)); then
      sleep "$DELAY"
    fi
  fi
done

exit "$status"
