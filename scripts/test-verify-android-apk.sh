#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Fixture checks for the release signer lines in scripts/verify-android-apk.sh.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SCRIPT="$ROOT/scripts/verify-android-apk.sh"

expect_ok() {
  local name="$1"
  local text="$2"
  if ! printf '%s\n' "$text" | "$SCRIPT" --release-certs-text >/dev/null; then
    echo "verify-android-apk certs $name: expected success" >&2
    exit 1
  fi
}

expect_fail() {
  local name="$1"
  local text="$2"
  local needle="$3"
  local err
  if err="$(printf '%s\n' "$text" | "$SCRIPT" --release-certs-text 2>&1)"; then
    echo "verify-android-apk certs $name: expected failure" >&2
    exit 1
  fi
  if ! printf '%s\n' "$err" | grep -q "$needle"; then
    echo "verify-android-apk certs $name: missing '$needle' in:" >&2
    printf '%s\n' "$err" >&2
    exit 1
  fi
}

expect_ok \
  "signer-1" \
  "Signer #1 certificate DN: CN=ice-box, OU=ice-box, O=ice-box, L=Unknown, ST=Unknown, C=US"
expect_ok \
  "v2-signer" \
  "V2 Signer: certificate DN: CN=ice-box, OU=ice-box, O=ice-box, L=Unknown, ST=Unknown, C=US"
expect_fail \
  "debug-signer-1" \
  "Signer #1 certificate DN: CN=Android Debug, O=Android, C=US" \
  "Android debug key"
expect_fail \
  "debug-v2" \
  "V2 Signer: certificate DN: CN=Android Debug, O=Android, C=US" \
  "Android debug key"
expect_fail \
  "no-signer" \
  "Verified using v2 scheme (APK Signature Scheme v2): true" \
  "no signer certificate"

echo "test-verify-android-apk: OK"
