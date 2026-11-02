#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Fixture checks for scripts/verify-signing-key.sh.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SCRIPT="$ROOT/scripts/verify-signing-key.sh"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/ice-box-signing-key-XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

fail() {
  echo "test-verify-signing-key: $*" >&2
  exit 1
}

python3 - "$TMP" <<'PY'
import base64
import hashlib
import json
import sys
from pathlib import Path

root = Path(sys.argv[1])
password = "test-password"
key_id = bytes.fromhex("94954A1C99C12378")[::-1]
secret_key = bytes(range(64))
salt = bytes(range(32))
opslimit = 1_048_576
memlimit = 33_554_432
checksum = hashlib.blake2b(b"Ed" + key_id + secret_key, digest_size=32).digest()


def wrap(text: str) -> str:
    return base64.b64encode(text.encode()).decode()


def secret_text(blob: bytes) -> str:
    return (
        "untrusted comment: rsign encrypted secret key\n"
        + base64.b64encode(blob).decode()
        + "\n"
    )


def public_config(keynum: bytes, name: str) -> None:
    public_blob = b"Ed" + keynum + bytes(range(32))
    text = (
        "untrusted comment: minisign public key: "
        + keynum[::-1].hex().upper()
        + "\n"
        + base64.b64encode(public_blob).decode()
        + "\n"
    )
    config = {"plugins": {"updater": {"pubkey": wrap(text)}}}
    (root / name).write_text(json.dumps(config), encoding="utf-8")


stream = hashlib.scrypt(
    password.encode(), salt=salt, n=32768, r=8, p=1, dklen=104, maxmem=memlimit * 2
)
encrypted = (
    b"Ed"
    + b"Sc"
    + b"B2"
    + salt
    + opslimit.to_bytes(8, "little")
    + memlimit.to_bytes(8, "little")
    + bytes(a ^ b for a, b in zip(key_id, stream[0:8]))
    + bytes(a ^ b for a, b in zip(secret_key, stream[8:72]))
    + bytes(a ^ b for a, b in zip(checksum, stream[72:104]))
)
encrypted_text = secret_text(encrypted)
(root / "encrypted.key").write_text(wrap(encrypted_text), encoding="utf-8")
(root / "encrypted.txt").write_text(encrypted_text, encoding="utf-8")

unencrypted = (
    b"Ed"
    + b"\x00\x00"
    + b"B2"
    + salt
    + (0).to_bytes(8, "little")
    + (0).to_bytes(8, "little")
    + key_id
    + secret_key
    + checksum
)
(root / "unencrypted.key").write_text(wrap(secret_text(unencrypted)), encoding="utf-8")

public_config(key_id, "match.conf.json")
public_config(bytes.fromhex("1111111111111111"), "wrong.conf.json")
PY

run() {
  local key="$1"
  local password="$2"
  local config="$3"
  TAURI_SIGNING_PRIVATE_KEY="$key" \
    TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$password" \
    bash "$SCRIPT" --pubkey-file "$config"
}

expect_ok() {
  local name="$1"
  shift
  if ! "$@" >/dev/null 2>&1; then
    echo "test-verify-signing-key: $name: expected success" >&2
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
    echo "test-verify-signing-key: $name: expected failure" >&2
    exit 1
  fi
  if ! printf '%s\n' "$err" | grep -q "$needle"; then
    echo "test-verify-signing-key: $name: missing '$needle' in:" >&2
    printf '%s\n' "$err" >&2
    exit 1
  fi
}

echo "== keys =="
expect_ok "encrypted" run "$(cat "$TMP/encrypted.key")" test-password "$TMP/match.conf.json"
expect_ok "raw-text" run "$(cat "$TMP/encrypted.txt")" test-password "$TMP/match.conf.json"
expect_ok "unencrypted" run "$(cat "$TMP/unencrypted.key")" "" "$TMP/match.conf.json"

echo "== failures =="
expect_fail "wrong-pubkey" "does not match" \
  run "$(cat "$TMP/encrypted.key")" test-password "$TMP/wrong.conf.json"
expect_fail "wrong-password" "wrong password" \
  run "$(cat "$TMP/encrypted.key")" not-the-password "$TMP/match.conf.json"
expect_fail "empty-key" "is empty" run "" test-password "$TMP/match.conf.json"

echo "test-verify-signing-key: OK"
