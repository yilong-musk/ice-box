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

expect_listing_ok() {
  local name="$1"
  local text="$2"
  if ! printf '%s\n' "$text" | "$SCRIPT" --release-listing-text >/dev/null; then
    echo "verify-android-apk listing $name: expected success" >&2
    exit 1
  fi
}

expect_listing_fail() {
  local name="$1"
  local text="$2"
  local needle="$3"
  local err
  if err="$(printf '%s\n' "$text" | "$SCRIPT" --release-listing-text 2>&1)"; then
    echo "verify-android-apk listing $name: expected failure" >&2
    exit 1
  fi
  if ! printf '%s\n' "$err" | grep -q "$needle"; then
    echo "verify-android-apk listing $name: missing '$needle' in:" >&2
    printf '%s\n' "$err" >&2
    exit 1
  fi
}

# The arm64 entry is first, then a long tail. A `grep -q` pipeline under
# pipefail treats that as a missing library because grep closes the pipe.
arm64_listing="    100  2026-09-30 00:00   lib/arm64-v8a/libice_box_mobile_lib.so"
i=0
while [[ "$i" -lt 4000 ]]; do
  arm64_listing+=$'\n'"    100  2026-09-30 00:00   assets/rule-set/geoip-${i}.srs"
  i=$((i + 1))
done
expect_listing_ok "arm64-with-long-tail" "$arm64_listing"
expect_listing_fail "missing-arm64" "    100  2026-09-30 00:00   assets/rule-set/geoip-cn.srs" "missing lib/arm64-v8a"
expect_listing_fail \
  "x86_64" \
  $'    100  2026-09-30 00:00   lib/arm64-v8a/libice.so\n    100  2026-09-30 00:00   lib/x86_64/libice.so' \
  "ABI other than arm64-v8a"

echo "test-verify-android-apk: OK"
