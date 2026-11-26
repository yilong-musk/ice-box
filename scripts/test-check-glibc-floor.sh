#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Fixture tests for scripts/check-glibc-floor.sh.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SCRIPT="$ROOT/scripts/check-glibc-floor.sh"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/ice-box-glibc-floor-XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

expect_ok() {
  local name="$1"
  shift
  if ! "$@" >/dev/null 2>&1; then
    echo "test-check-glibc-floor: $name: expected success" >&2
    "$@" >&2 || true
    exit 1
  fi
}

expect_fail() {
  local name="$1"
  local needle="$2"
  shift 2
  local err
  if err="$("$@" 2>&1)"; then
    echo "test-check-glibc-floor: $name: expected failure" >&2
    exit 1
  fi
  if ! printf '%s\n' "$err" | grep -q "$needle"; then
    echo "test-check-glibc-floor: $name: missing '$needle' in:" >&2
    printf '%s\n' "$err" >&2
    exit 1
  fi
}

run_parse() {
  local max_version="$1"
  local label="$2"
  local payload="$3"
  printf '%s' "$payload" | bash "$SCRIPT" --parse-stdin "$max_version" "$label"
}

echo "== parser unit tests (--parse-stdin) =="

# Synthetic readelf -V with non-weak GLIBC_2.35 <= 2.35 -> pass.
FIXTURE_AT_FLOOR="$(cat <<'EOF'
Version symbols section '.gnu.version' contains 4 entries:
  000:   0 (*local*)       2 (GLIBC_2.39)

Version needs section '.gnu.version_r' contains 1 entry:
 Addr: 0x0000000000001000  Offset: 0x00001000  Link: 7 (.dynstr)
  000000: Version: 1  File: libc.so.6  Cnt: 2
  0x0010:   Name: GLIBC_2.2.5  Flags: none  Version: 3
  0x0020:   Name: GLIBC_2.35  Flags: none  Version: 2
EOF
)"
expect_ok "at-floor-2.35" run_parse 2.35 "at-floor" "$FIXTURE_AT_FLOOR"

# Weak GLIBC_2.39 entry must be allowed under 2.35 floor.
FIXTURE_WEAK="$(cat <<'EOF'
Version needs section '.gnu.version_r' contains 1 entry:
  000000: Version: 1  File: libc.so.6  Cnt: 3
  0x0010:   Name: GLIBC_2.34  Flags: none  Version: 4
  0x0020:   Name: GLIBC_2.39  Flags: WEAK  Version: 5
  0x0030:   Name: GLIBC_2.40  Flags: BASE | WEAK  Version: 6
EOF
)"
expect_ok "weak-2.39-allowed" run_parse 2.35 "weak-2.39" "$FIXTURE_WEAK"

# Non-weak GLIBC_2.39 must fail under 2.35 floor.
FIXTURE_TOO_NEW="$(cat <<'EOF'
Version needs section '.gnu.version_r' contains 1 entry:
  000000: Version: 1  File: libc.so.6  Cnt: 2
  0x0010:   Name: GLIBC_2.35  Flags: none  Version: 2
  0x0020:   Name: GLIBC_2.39  Flags: none  Version: 21
EOF
)"
expect_fail "non-weak-2.39-rejected" "GLIBC_2.39" \
  run_parse 2.35 "too-new" "$FIXTURE_TOO_NEW"

# Numeric (not lexical) version comparison: 2.10 > 2.9 must fail.
FIXTURE_NUMERIC="$(cat <<'EOF'
Version needs section '.gnu.version_r' contains 1 entry:
  000000: Version: 1  File: libc.so.6  Cnt: 1
  0x0010:   Name: GLIBC_2.10  Flags: none  Version: 2
EOF
)"
expect_fail "numeric-2.10-vs-2.9" "GLIBC_2.10" \
  run_parse 2.9 "numeric" "$FIXTURE_NUMERIC"
expect_ok "numeric-2.9-vs-2.10" run_parse 2.10 "numeric-ok" "$(cat <<'EOF'
  0x0010:   Name: GLIBC_2.9  Flags: none  Version: 2
EOF
)"

# Trailing zero equivalence: 2.35.0 <= 2.35 passes; 2.35.1 > 2.35 fails.
expect_ok "trailing-zero-2.35.0" run_parse 2.35 "zero" "$(cat <<'EOF'
  0x0010:   Name: GLIBC_2.35.0  Flags: none  Version: 2
EOF
)"
expect_fail "patch-2.35.1-rejected" "GLIBC_2.35.1" run_parse 2.35 "patch" "$(cat <<'EOF'
  0x0010:   Name: GLIBC_2.35.1  Flags: none  Version: 2
EOF
)"

# Empty / static binary (no version needs) -> pass.
expect_ok "empty-static" run_parse 2.35 "empty" ""
expect_ok "no-version-info" run_parse 2.35 "static" \
  $'No version information found in this file.\n'

# Non-GLIBC libraries above 2.35 are ignored.
expect_ok "non-glibc-ignored" run_parse 2.35 "other-lib" "$(cat <<'EOF'
Version needs section '.gnu.version_r' contains 1 entry:
  000000: Version: 1  File: libgcc_s.so.1  Cnt: 1
  0x0010:   Name: GCC_4.2.0  Flags: none  Version: 2
EOF
)"

echo "== argument validation =="
expect_fail "invalid-version" "invalid max version" \
  bash "$SCRIPT" not-a-version /bin/ls
expect_fail "missing-file" "file not found" \
  bash "$SCRIPT" 2.35 "$TMP/does-not-exist"

echo "== host ELF tests =="
if [[ "$(uname -s)" == "Linux" ]] && { command -v readelf >/dev/null 2>&1 || command -v objdump >/dev/null 2>&1; }; then
  HOST_BIN="/bin/ls"
  if [[ ! -f "$HOST_BIN" ]]; then
    HOST_BIN="/bin/sh"
  fi
  expect_ok "host-bin-high-floor" bash "$SCRIPT" 99.0 "$HOST_BIN"
  expect_fail "host-bin-low-floor" "FAIL" bash "$SCRIPT" 2.0 "$HOST_BIN"

  # Non-ELF file must fail rather than passing as static.
  printf 'not an elf file\n' >"$TMP/plain.txt"
  expect_fail "non-elf-rejected" "not an ELF binary" \
    bash "$SCRIPT" 2.35 "$TMP/plain.txt"

  # Minimal valid 64-byte static ELF64 header with zero section headers -> pass.
  python3 - "$TMP/static.elf" <<'PY'
import struct
import sys

# ELF64 little-endian executable header with 0 program/section headers.
header = struct.pack(
    "<16sHHIQQQIHHHHHH",
    b"\x7fELF\x02\x01\x01" + b"\x00" * 9,
    2,     # ET_EXEC
    62,    # EM_X86_64
    1,     # EV_CURRENT
    0,     # e_entry
    0,     # e_phoff
    0,     # e_shoff
    0,     # e_flags
    64,    # e_ehsize
    56,    # e_phentsize
    0,     # e_phnum
    64,    # e_shentsize
    0,     # e_shnum
    0,     # e_shstrndx
)
with open(sys.argv[1], "wb") as handle:
    handle.write(header)
PY
  expect_ok "minimal-static-elf" bash "$SCRIPT" 2.35 "$TMP/static.elf"

  # If the bundled static Go sing-box binary is present locally, check it too.
  for sb in "$ROOT"/third_party/sing-box/linux-*/sing-box; do
    if [[ -f "$sb" ]]; then
      expect_ok "bundled-sing-box" bash "$SCRIPT" 2.35 "$sb"
    fi
  done
else
  echo "note: skipping host ELF tests on non-Linux host ($(uname -s))"
fi

echo "test-check-glibc-floor: OK"
