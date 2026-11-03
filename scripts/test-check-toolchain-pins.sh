#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Fixture checks for scripts/check-toolchain-pins.sh.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SCRIPT="$ROOT/scripts/check-toolchain-pins.sh"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/ice-box-toolchain-pins-XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

fail() {
  echo "test-check-toolchain-pins: $*" >&2
  exit 1
}

# write_workflow <yaml>: a fixture repo with one workflow and the 1.94.1 pin.
write_workflow() {
  rm -rf "$TMP/repo"
  mkdir -p "$TMP/repo/.github/workflows"
  printf '[toolchain]\nchannel = "1.94.1"\n' >"$TMP/repo/rust-toolchain.toml"
  cat >"$TMP/repo/.github/workflows/ci.yml"
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
write_workflow <<'EOF'
jobs:
  a:
    steps:
      - uses: dtolnay/rust-toolchain@stable
        with:
          toolchain: "1.94.1"
          components: rustfmt
      - uses: dtolnay/rust-toolchain@1.94.1
      - uses: dtolnay/rust-toolchain@master
        with:
          toolchain: 1.94.1
      - run: rustup toolchain install 1.94.1
EOF
expect_ok all-forms
bash "$SCRIPT" "$TMP/repo" | grep -Fq "4 pins" || fail "all-forms: expected 4 pins"

echo "== failures =="
write_workflow <<'EOF'
jobs:
  a:
    steps:
      - uses: dtolnay/rust-toolchain@stable
        with:
          toolchain: 1.95.0
EOF
expect_fail unquoted "pins 1.95.0"

write_workflow <<'EOF'
jobs:
  a:
    steps:
      - uses: dtolnay/rust-toolchain@1.95.0
EOF
expect_fail version-ref "pins 1.95.0"

write_workflow <<'EOF'
jobs:
  a:
    steps:
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy
      - name: next step
        with:
          toolchain: "1.94.1"
EOF
expect_fail floating "without a toolchain input"

write_workflow <<'EOF'
jobs:
  a:
    steps:
      - uses: dtolnay/rust-toolchain@stable
        with:
          toolchain: '1.94.1'
      - run: rustup default 1.93.0
EOF
expect_fail rustup "pins 1.93.0"

write_workflow <<'EOF'
jobs:
  a:
    steps:
      - run: cargo build
EOF
expect_fail none "no toolchain pins"

echo "== repository =="
bash "$SCRIPT" >/dev/null || fail "the repository's own pins do not match"

echo "test-check-toolchain-pins: OK"
