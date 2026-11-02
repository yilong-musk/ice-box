#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Verify that TAURI_SIGNING_PRIVATE_KEY matches the committed updater pubkey.
# Usage: scripts/verify-signing-key.sh [--pubkey-file <path>]
#
# Reads TAURI_SIGNING_PRIVATE_KEY (the base64 key-file content, the raw
# minisign text, or a path) and TAURI_SIGNING_PRIVATE_KEY_PASSWORD, decrypts
# the minisign secret key, and compares its key id with
# plugins.updater.pubkey. A wrong key fails the preflight instead of every
# installed client later.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PUBKEY_FILE="$ROOT/apps/desktop/src-tauri/tauri.conf.json"

usage() {
  cat >&2 <<'EOF'
usage: scripts/verify-signing-key.sh [--pubkey-file <path>]
EOF
}

while (($# > 0)); do
  case "$1" in
  --pubkey-file)
    PUBKEY_FILE="${2:-}"
    if [[ -z "$PUBKEY_FILE" ]]; then
      echo "--pubkey-file needs a path" >&2
      exit 2
    fi
    shift 2
    ;;
  -h | --help)
    usage
    exit 0
    ;;
  *)
    echo "unknown argument: $1" >&2
    usage
    exit 2
    ;;
  esac
done

if [[ ! -f "$PUBKEY_FILE" ]]; then
  echo "pubkey source not found: $PUBKEY_FILE" >&2
  exit 1
fi

python3 - "$PUBKEY_FILE" <<'PY'
import base64
import binascii
import hashlib
import json
import os
import sys

pubkey_path = sys.argv[1]


def fail(message: str) -> None:
    print(message, file=sys.stderr)
    sys.exit(1)


def decode_base64(value: str, label: str) -> bytes:
    try:
        return base64.b64decode(value, validate=True)
    except (binascii.Error, ValueError) as error:
        fail(f"{label}: invalid base64 ({error})")


def key_text(value: str, label: str) -> str:
    text = value.strip()
    if "untrusted comment:" in text:
        return text
    try:
        decoded = base64.b64decode("".join(text.split()), validate=True).decode("utf-8")
    except (binascii.Error, UnicodeDecodeError):
        fail(f"{label}: not a minisign key")
    if "untrusted comment:" not in decoded:
        fail(f"{label}: not a minisign key")
    return decoded


def key_blob(text: str, label: str) -> bytes:
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith("untrusted comment:"):
            continue
        return decode_base64(line, label)
    fail(f"{label}: no key data")


try:
    config = json.load(open(pubkey_path, encoding="utf-8"))
    encoded = config["plugins"]["updater"]["pubkey"]
except (OSError, json.JSONDecodeError, KeyError, TypeError) as error:
    fail(f"{pubkey_path}: cannot read plugins.updater.pubkey ({error})")
pub = key_blob(key_text(encoded, f"{pubkey_path}: pubkey"), f"{pubkey_path}: pubkey")
if len(pub) != 42 or pub[:2] != b"Ed":
    fail(f"{pubkey_path}: public key has {len(pub)} bytes, expected a 42-byte minisign key")
pub_key_id = pub[2:10]

raw = os.environ.get("TAURI_SIGNING_PRIVATE_KEY", "")
if os.path.isfile(raw):
    raw = open(raw, encoding="utf-8").read()
if not raw.strip():
    fail("TAURI_SIGNING_PRIVATE_KEY is empty")
password = os.environ.get("TAURI_SIGNING_PRIVATE_KEY_PASSWORD", "")

blob = key_blob(key_text(raw, "TAURI_SIGNING_PRIVATE_KEY"), "TAURI_SIGNING_PRIVATE_KEY")
if len(blob) != 158:
    fail(
        "TAURI_SIGNING_PRIVATE_KEY: "
        f"{len(blob)} bytes, expected a 158-byte minisign secret key"
    )
if blob[:2] != b"Ed":
    fail("TAURI_SIGNING_PRIVATE_KEY: unexpected signature algorithm")

sig_alg, kdf_alg, chk_alg = blob[0:2], blob[2:4], blob[4:6]
salt = blob[6:38]
stored_opslimit = int.from_bytes(blob[38:46], "little")
stored_memlimit = int.from_bytes(blob[46:54], "little")
key_id = bytearray(blob[54:62])
secret_key = bytearray(blob[62:126])
checksum = bytearray(blob[126:158])

if kdf_alg == b"\x00\x00":
    pass
elif kdf_alg == b"Sc":
    # Mirror rsign's raw_scrypt_params() so the key can be decrypted without
    # the Tauri CLI; the stream XORs the key id, secret key, and checksum.
    if stored_memlimit > 1_073_741_824:
        fail("TAURI_SIGNING_PRIVATE_KEY: scrypt parameters too high")
    opslimit = max(32768, stored_opslimit)
    r = 8
    n_log2 = 1
    if opslimit < stored_memlimit // 32:
        maxn = opslimit // (r * 4)
    else:
        maxn = stored_memlimit // (r * 128)
    while n_log2 < 63:
        if (1 << n_log2) > maxn // 2:
            break
        n_log2 += 1
    if n_log2 > 20:
        fail("TAURI_SIGNING_PRIVATE_KEY: scrypt parameters too high")
    if opslimit < stored_memlimit // 32:
        p = 1
    else:
        maxrp = min(0x3FFF_FFFF, (opslimit // 4) // (1 << n_log2))
        p = maxrp // r
    n = 1 << n_log2
    try:
        stream = hashlib.scrypt(
            password.encode(),
            salt=salt,
            n=n,
            r=r,
            p=p,
            dklen=8 + 64 + 32,
            maxmem=max(stored_memlimit, 128 * r * n * 2),
        )
    except (ValueError, OSError) as error:
        fail(f"TAURI_SIGNING_PRIVATE_KEY: cannot decrypt ({error})")
    for index in range(8):
        key_id[index] ^= stream[index]
    for index in range(64):
        secret_key[index] ^= stream[8 + index]
    for index in range(32):
        checksum[index] ^= stream[72 + index]
else:
    fail(f"TAURI_SIGNING_PRIVATE_KEY: unsupported KDF algorithm {kdf_alg!r}")

if chk_alg != b"B2":
    fail(f"TAURI_SIGNING_PRIVATE_KEY: unsupported checksum algorithm {chk_alg!r}")
digest = hashlib.blake2b(bytes(sig_alg + key_id + secret_key), digest_size=32).digest()
if digest != bytes(checksum):
    fail("TAURI_SIGNING_PRIVATE_KEY: wrong password or corrupt key (checksum mismatch)")

if bytes(key_id) != pub_key_id:
    fail(
        "signing key does not match plugins.updater.pubkey: "
        f"private {bytes(key_id[::-1]).hex().upper()} vs "
        f"pubkey {pub_key_id[::-1].hex().upper()}"
    )

print(f"verify-signing-key: OK (key id {pub_key_id[::-1].hex().upper()})")
PY
