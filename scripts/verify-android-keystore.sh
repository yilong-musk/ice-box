#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Verify the Android release keystore and its passwords before a release.
# Usage: scripts/verify-android-keystore.sh [--keystore <path>]
#
# Reads ANDROID_KEYSTORE_BASE64 (unless --keystore is given),
# ANDROID_KEYSTORE_PASSWORD, ANDROID_KEY_ALIAS, and ANDROID_KEY_PASSWORD.
# The base64 value is materialized into a private temporary file that is
# removed on exit. Requires keytool from a JDK; release.yml installs one with
# actions/setup-java.
set -euo pipefail

TMP="$(mktemp -d "${TMPDIR:-/tmp}/ice-box-keystore-XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

usage() {
  cat >&2 <<'EOF'
usage: scripts/verify-android-keystore.sh [--keystore <path>]

  --keystore <path>   use a local keystore instead of ANDROID_KEYSTORE_BASE64

Environment:
  ANDROID_KEYSTORE_BASE64    base64 of the .jks file (unless --keystore)
  ANDROID_KEYSTORE_PASSWORD  store password
  ANDROID_KEY_ALIAS          key alias
  ANDROID_KEY_PASSWORD       key password
EOF
}

KEYSTORE=""
while (($# > 0)); do
  case "$1" in
  --keystore)
    KEYSTORE="${2:-}"
    if [[ -z "$KEYSTORE" ]]; then
      echo "--keystore needs a path" >&2
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

for name in ANDROID_KEYSTORE_PASSWORD ANDROID_KEY_ALIAS ANDROID_KEY_PASSWORD; do
  if [[ -z "${!name:-}" ]]; then
    echo "$name is empty" >&2
    exit 1
  fi
done

if [[ -z "$KEYSTORE" ]]; then
  if [[ -z "${ANDROID_KEYSTORE_BASE64:-}" ]]; then
    echo "ANDROID_KEYSTORE_BASE64 is empty and --keystore was not given" >&2
    exit 1
  fi
  KEYSTORE="$TMP/release.jks"
  python3 - "$ANDROID_KEYSTORE_BASE64" "$KEYSTORE" <<'PY'
import base64
import binascii
import sys

value = "".join(sys.argv[1].split())
try:
    data = base64.b64decode(value, validate=True)
except (binascii.Error, ValueError) as error:
    raise SystemExit(f"ANDROID_KEYSTORE_BASE64 is not valid base64: {error}")
if not data:
    raise SystemExit("ANDROID_KEYSTORE_BASE64 decoded to an empty file")
with open(sys.argv[2], "wb") as handle:
    handle.write(data)
PY
  chmod 600 "$KEYSTORE"
fi

if [[ ! -f "$KEYSTORE" ]]; then
  echo "keystore not found: $KEYSTORE" >&2
  exit 1
fi
if ! command -v keytool >/dev/null 2>&1; then
  echo "keytool is required (install a JDK; release.yml uses actions/setup-java)" >&2
  exit 1
fi

if ! keytool -list -keystore "$KEYSTORE" \
  -storepass:env ANDROID_KEYSTORE_PASSWORD -alias "$ANDROID_KEY_ALIAS" \
  >/dev/null 2>"$TMP/keytool-list.err"; then
  cat "$TMP/keytool-list.err" >&2
  echo "Android keystore, store password, or alias is invalid" >&2
  exit 1
fi

# -certreq decrypts the private key entry, so it also validates the key
# password. It does not modify the keystore.
if ! keytool -certreq -alias "$ANDROID_KEY_ALIAS" -keystore "$KEYSTORE" \
  -storepass:env ANDROID_KEYSTORE_PASSWORD -keypass:env ANDROID_KEY_PASSWORD \
  -file "$TMP/request.csr" >/dev/null 2>"$TMP/keytool-key.err"; then
  cat "$TMP/keytool-key.err" >&2
  echo "ANDROID_KEY_PASSWORD is wrong or '$ANDROID_KEY_ALIAS' is not a private key" >&2
  exit 1
fi

echo "verify-android-keystore: OK (alias $ANDROID_KEY_ALIAS)"
