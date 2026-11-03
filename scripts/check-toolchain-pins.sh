#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Check that every `toolchain: "x.y.z"` pin in .github/ matches the channel in
# rust-toolchain.toml, so CI, the release builds, and local builds use one
# Rust version. Bump rust-toolchain.toml and the workflow pins together.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

channel="$(sed -n 's/^channel = "\([^"]*\)"$/\1/p' rust-toolchain.toml)"
if [[ -z "$channel" ]]; then
  echo "check-toolchain-pins: no channel in rust-toolchain.toml" >&2
  exit 1
fi

status=0
found=0
while IFS= read -r hit; do
  found=1
  pin="$(printf '%s\n' "$hit" | sed -n 's/.*toolchain: *"\([^"]*\)".*/\1/p')"
  if [[ "$pin" != "$channel" ]]; then
    echo "check-toolchain-pins: ${hit%%:*:*} pins $pin; rust-toolchain.toml has $channel" >&2
    echo "  $hit" >&2
    status=1
  fi
done < <(grep -rnE '^\s*toolchain: *"' .github || true)

if [[ "$found" == 0 ]]; then
  echo "check-toolchain-pins: no toolchain pins found under .github/" >&2
  exit 1
fi
[[ "$status" == 0 ]] && echo "check-toolchain-pins: OK ($channel)"
exit "$status"
