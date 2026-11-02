#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Check that Tauri updater signatures use the committed pubkey's key id.
# Usage: scripts/verify-updater-signatures.sh [--pubkey-file <path>] <assets-dir>
#
# <assets-dir> must contain exactly one macOS *.app.tar.gz.sig and one Windows
# *.exe.sig, plus their artifacts. The default pubkey source is the committed
# apps/desktop/src-tauri/tauri.conf.json; a fixture may pass a JSON file with the
# same shape, or a decoded minisign public-key file.
#
# The updater verifies signatures cryptographically on the client. This check
# catches the failure CI can prevent: a release signed with a different
# minisign key than plugins.updater.pubkey, which would leave every installed
# client unable to install the update. It compares key ids; it does not
# re-verify the signature itself.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PUBKEY_FILE="$ROOT/apps/desktop/src-tauri/tauri.conf.json"
ASSETS=""

usage() {
  cat >&2 <<'EOF'
usage: scripts/verify-updater-signatures.sh [--pubkey-file <path>] <assets-dir>
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
    if [[ -n "$ASSETS" ]]; then
      echo "unexpected argument: $1" >&2
      usage
      exit 2
    fi
    ASSETS="$1"
    shift
    ;;
  esac
done

if [[ -z "$ASSETS" ]]; then
  usage
  exit 2
fi
if [[ ! -d "$ASSETS" ]]; then
  echo "assets dir not found: $ASSETS" >&2
  exit 1
fi
if [[ ! -f "$PUBKEY_FILE" ]]; then
  echo "pubkey source not found: $PUBKEY_FILE" >&2
  exit 1
fi

python3 - "$PUBKEY_FILE" "$ASSETS" <<'PY'
import base64
import binascii
import json
import re
import sys
from pathlib import Path

pubkey_path = Path(sys.argv[1])
assets = Path(sys.argv[2])


def fail(message: str) -> None:
    print(message, file=sys.stderr)
    sys.exit(1)


def decode_base64(value: str, label: str) -> bytes:
    try:
        return base64.b64decode(value, validate=True)
    except (binascii.Error, ValueError) as error:
        fail(f"{label}: invalid base64 ({error})")


source = pubkey_path.read_text(encoding="utf-8")
if pubkey_path.suffix == ".json":
    try:
        encoded = json.loads(source)["plugins"]["updater"]["pubkey"]
    except (json.JSONDecodeError, KeyError, TypeError) as error:
        fail(f"{pubkey_path}: cannot read plugins.updater.pubkey ({error})")
    source = decode_base64(encoded, f"{pubkey_path}: pubkey").decode("utf-8")

comment_id = None
comment = re.search(r"minisign public key:\s*([0-9A-Fa-f]{16})", source)
if comment:
    comment_id = comment.group(1).upper()

key_line = ""
for line in source.splitlines():
    line = line.strip()
    if not line or line.startswith("untrusted comment:"):
        continue
    key_line = line
    break
if not key_line:
    fail(f"{pubkey_path}: no minisign public key data")

key = decode_base64(key_line, f"{pubkey_path}: public key")
if len(key) != 42:
    fail(f"{pubkey_path}: public key has {len(key)} bytes, expected 42")
key_id_bytes = key[2:10]
key_id = key_id_bytes.hex().upper()
# Minisign prints the key id with the bytes reversed relative to the key data.
display_id = key_id_bytes[::-1].hex().upper()
if comment_id and comment_id not in (key_id, display_id):
    fail(
        f"{pubkey_path}: comment key id {comment_id} does not match "
        f"public key data {display_id}"
    )

mac_sigs = sorted(p for p in assets.glob("*.app.tar.gz.sig") if p.is_file())
win_sigs = sorted(p for p in assets.glob("*.exe.sig") if p.is_file())
if len(mac_sigs) != 1:
    fail(f"expected exactly one macOS .app.tar.gz.sig in {assets}, found {len(mac_sigs)}")
if len(win_sigs) != 1:
    fail(f"expected exactly one Windows .exe.sig in {assets}, found {len(win_sigs)}")

for signature in mac_sigs + win_sigs:
    artifact = Path(str(signature)[: -len(".sig")])
    if not artifact.is_file():
        fail(f"{signature.name}: missing artifact {artifact.name}")
    if artifact.stat().st_size == 0:
        fail(f"{artifact.name}: empty artifact")

    raw = signature.read_text(encoding="utf-8").strip()
    # Tauri writes the .sig file as base64 of the minisign signature text.
    # Accept plain minisign text too so the check also works on a hand-made
    # signature produced by the minisign CLI.
    if "untrusted comment:" not in raw:
        try:
            decoded = base64.b64decode("".join(raw.split()), validate=True).decode("utf-8")
        except (binascii.Error, UnicodeDecodeError):
            fail(f"{signature.name}: not a Tauri minisign signature")
        if "untrusted comment:" in decoded:
            raw = decoded

    data_line = ""
    for line in raw.splitlines():
        line = line.strip()
        if not line or line.startswith("untrusted comment:") or line.startswith("trusted comment:"):
            continue
        data_line = line
        break
    if not data_line:
        fail(f"{signature.name}: no signature data")

    blob = decode_base64(data_line, f"{signature.name}: signature")
    if len(blob) != 74:
        fail(f"{signature.name}: signature has {len(blob)} bytes, expected 74")
    signature_id = blob[2:10].hex().upper()
    if signature_id != key_id:
        fail(
            f"{signature.name}: key id {blob[2:10][::-1].hex().upper()} does not match "
            f"plugins.updater.pubkey {display_id}"
        )
    print(f"{signature.name}: key id {display_id} matches the updater pubkey")

print("verify-updater-signatures: OK")
PY
