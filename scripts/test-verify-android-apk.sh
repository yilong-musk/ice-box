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

# `unzip -v` rows: Length Method Size Cmpr Date Time CRC-32 Name.
row() {
  printf '%10s  %-6s %8s %3s  2026-09-30 00:00 00000000  %s' "$1" "$2" "$3" "$4" "$5"
}
header=" Length   Method    Size  Cmpr    Date    Time   CRC-32   Name"

# The arm64 entry is first, then a long tail. A `grep -q` pipeline under
# pipefail treats that as a missing library because grep closes the pipe.
arm64_listing="$header"$'\n'"$(row 8858176 Defl:N 3958209 55% lib/arm64-v8a/libice_box_mobile_lib.so)"
i=0
while [[ "$i" -lt 4000 ]]; do
  arm64_listing+=$'\n'"$(row 100 Stored 100 0% "assets/rule-set/geoip-${i}.srs")"
  i=$((i + 1))
done
expect_listing_ok "arm64-with-long-tail" "$arm64_listing"
expect_listing_ok \
  "stored-arsc" \
  "$header"$'\n'"$(row 61721672 Defl:N 19576911 68% lib/arm64-v8a/libbox.so)"$'\n'"$(row 524388 Stored 524388 0% resources.arsc)"
expect_listing_fail \
  "missing-arm64" \
  "$header"$'\n'"$(row 100 Defl:N 50 50% assets/rule-set/geoip-cn.srs)" \
  "missing lib/arm64-v8a"
expect_listing_fail \
  "x86_64" \
  "$header"$'\n'"$(row 100 Defl:N 50 50% lib/arm64-v8a/libice.so)"$'\n'"$(row 100 Defl:N 50 50% lib/x86_64/libice.so)" \
  "ABI other than arm64-v8a"
# useLegacyPackaging dropped: AGP stores the .so files for mmap loading.
expect_listing_fail \
  "stored-so" \
  "$header"$'\n'"$(row 8858176 Defl:N 3958209 55% lib/arm64-v8a/libice_box_mobile_lib.so)"$'\n'"$(row 61721672 Stored 61721672 0% lib/arm64-v8a/libbox.so)" \
  "uncompressed"
# `unzip -l` has no method column, so compression cannot be checked.
expect_listing_fail \
  "unzip-l" \
  "    100  2026-09-30 00:00   lib/arm64-v8a/libice_box_mobile_lib.so" \
  "expected unzip -v"

expect_locales_ok() {
  local name="$1"
  local text="$2"
  if ! printf '%s\n' "$text" | "$SCRIPT" --release-locales-text >/dev/null; then
    echo "verify-android-apk locales $name: expected success" >&2
    exit 1
  fi
}

expect_locales_fail() {
  local name="$1"
  local text="$2"
  local needle="$3"
  local err
  if err="$(printf '%s\n' "$text" | "$SCRIPT" --release-locales-text 2>&1)"; then
    echo "verify-android-apk locales $name: expected failure" >&2
    exit 1
  fi
  if ! printf '%s\n' "$err" | grep -q "$needle"; then
    echo "verify-android-apk locales $name: missing '$needle' in:" >&2
    printf '%s\n' "$err" >&2
    exit 1
  fi
}

pkg="package: name='com.yilongmusk.icebox' versionCode='2009' versionName='0.2.9'"
# Output of a real filtered release APK (aapt2 35.0.0).
expect_locales_ok "filtered" "$pkg"$'\n'"locales: '--_--' 'zh-CN' 'zh-HK' 'zh-TW'"
expect_locales_ok "with-en" "$pkg"$'\n'"locales: '--_--' 'en' 'en-GB' 'zh-CN' 'zh-TW'"
# localeFilters dropped: AndroidX/Material bring back dozens of locales.
expect_locales_fail \
  "unfiltered" \
  "$pkg"$'\n'"locales: '--_--' 'af' 'am' 'ar' 'zh-CN' 'zh-HK' 'zh-TW'" \
  "outside en/zh-CN/zh-TW/zh-HK: af am ar"
expect_locales_fail "no-line" "$pkg" "no locales line"

expect_version_ok() {
  local name="$1"
  local version="$2"
  local text="$3"
  if ! printf '%s\n' "$text" | "$SCRIPT" --release-badging-text "$version" >/dev/null; then
    echo "verify-android-apk version $name: expected success" >&2
    exit 1
  fi
}

expect_version_fail() {
  local name="$1"
  local version="$2"
  local text="$3"
  local needle="$4"
  local err
  if err="$(printf '%s\n' "$text" | "$SCRIPT" --release-badging-text "$version" 2>&1)"; then
    echo "verify-android-apk version $name: expected failure" >&2
    exit 1
  fi
  if ! printf '%s\n' "$err" | grep -q "$needle"; then
    echo "verify-android-apk version $name: missing '$needle' in:" >&2
    printf '%s\n' "$err" >&2
    exit 1
  fi
}

expect_version_ok \
  "match" \
  "0.2.6" \
  "package: name='com.yilong-musk.icebox' versionCode='2006' versionName='0.2.6' platformBuildVersionName='15'"
expect_version_ok \
  "prerelease" \
  "0.3.0-rc.1" \
  "package: name='com.yilong-musk.icebox' versionCode='3000' versionName='0.3.0-rc.1' platformBuildVersionName='15'"
expect_version_fail \
  "mismatch" \
  "0.2.6" \
  "package: name='com.yilong-musk.icebox' versionCode='2005' versionName='0.2.5' platformBuildVersionName='15'" \
  "does not match"
expect_version_fail \
  "version-code-mismatch" \
  "0.2.6" \
  "package: name='com.yilong-musk.icebox' versionCode='2005' versionName='0.2.6' platformBuildVersionName='15'" \
  "versionCode"
expect_version_fail \
  "missing-version" \
  "0.2.6" \
  "package: name='com.yilong-musk.icebox' versionCode='2006' platformBuildVersionName='15'" \
  "does not match"
expect_version_fail \
  "missing-version-code" \
  "0.2.6" \
  "package: name='com.yilong-musk.icebox' versionName='0.2.6' platformBuildVersionName='15'" \
  "versionCode"
expect_version_fail \
  "invalid-version" \
  "not-a-version" \
  "package: name='com.yilong-musk.icebox' versionCode='2006' versionName='not-a-version' platformBuildVersionName='15'" \
  "not a valid version"

echo "test-verify-android-apk: OK"
