#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Fixture checks for scripts/verify-android-keystore.sh.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SCRIPT="$ROOT/scripts/verify-android-keystore.sh"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/ice-box-keystore-fixture-XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

fail() {
  echo "test-verify-android-keystore: $*" >&2
  exit 1
}

mkdir -p "$TMP/bin"
cat >"$TMP/bin/keytool" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
alias=""
file=""
certreq=0
while (($# > 0)); do
  case "$1" in
  -alias)
    alias="${2:-}"
    shift 2
    ;;
  -file)
    file="${2:-}"
    shift 2
    ;;
  -certreq)
    certreq=1
    shift
    ;;
  *)
    shift
    ;;
  esac
done
if [[ "${ANDROID_KEYSTORE_PASSWORD:-}" != "store-pass" ]]; then
  echo "Keystore was tampered with, or password was incorrect" >&2
  exit 1
fi
if [[ "$alias" != "ice-box" ]]; then
  echo "Alias <$alias> does not exist" >&2
  exit 1
fi
if [[ "$certreq" == "1" ]]; then
  if [[ "${ANDROID_KEY_PASSWORD:-}" != "key-pass" ]]; then
    echo "Cannot recover key" >&2
    exit 1
  fi
  if [[ -n "$file" ]]; then
    printf 'csr\n' >"$file"
  fi
  exit 0
fi
echo "ice-box, PrivateKeyEntry,"
EOF
chmod +x "$TMP/bin/keytool"

BASE64="$(python3 -c 'import base64; print(base64.b64encode(b"fake-jks").decode())')"

run() {
  local base64="$1"
  local store="$2"
  local alias="$3"
  local key="$4"
  PATH="$TMP/bin:$PATH" \
    ANDROID_KEYSTORE_BASE64="$base64" \
    ANDROID_KEYSTORE_PASSWORD="$store" \
    ANDROID_KEY_ALIAS="$alias" \
    ANDROID_KEY_PASSWORD="$key" \
    bash "$SCRIPT"
}

expect_ok() {
  local name="$1"
  shift
  if ! "$@" >/dev/null 2>&1; then
    echo "test-verify-android-keystore: $name: expected success" >&2
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
    echo "test-verify-android-keystore: $name: expected failure" >&2
    exit 1
  fi
  if ! printf '%s\n' "$err" | grep -q "$needle"; then
    echo "test-verify-android-keystore: $name: missing '$needle' in:" >&2
    printf '%s\n' "$err" >&2
    exit 1
  fi
}

echo "== keystore checks =="
expect_ok "valid" run "$BASE64" store-pass ice-box key-pass
printf 'local-jks\n' >"$TMP/local.jks"
expect_ok "local-file" env PATH="$TMP/bin:$PATH" \
  ANDROID_KEYSTORE_PASSWORD=store-pass ANDROID_KEY_ALIAS=ice-box \
  ANDROID_KEY_PASSWORD=key-pass bash "$SCRIPT" --keystore "$TMP/local.jks"

echo "== failures =="
expect_fail "wrong-store-password" "store password" \
  run "$BASE64" wrong-pass ice-box key-pass
expect_fail "wrong-alias" "alias" run "$BASE64" store-pass other-alias key-pass
expect_fail "wrong-key-password" "ANDROID_KEY_PASSWORD is wrong" \
  run "$BASE64" store-pass ice-box wrong-pass
expect_fail "invalid-base64" "base64" run "not base64!" store-pass ice-box key-pass
expect_fail "missing-base64" "ANDROID_KEYSTORE_BASE64 is empty" \
  run "" store-pass ice-box key-pass
expect_fail "missing-alias" "ANDROID_KEY_ALIAS is empty" \
  run "$BASE64" store-pass "" key-pass

echo "test-verify-android-keystore: OK"
