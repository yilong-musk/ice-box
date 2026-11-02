#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Check an ice-box APK signature.
# Usage: scripts/verify-android-apk.sh [--release] [--version <ver>] <apk>
#
# Without --release, the APK must verify (the CI debug APK is signed with
# the Android debug key). With --release, the signer must not be that debug
# key, and the native libraries must be arm64-v8a only.
#
# --version checks the APK manifest versionName and versionCode against the
# release version (the versionCode comes from scripts/android-version-code.sh).
#
# --release-certs-text reads `apksigner verify --print-certs` output on stdin
# and applies only the release signer checks. --release-listing-text does the
# same for `unzip -l` output, and --release-badging-text <ver> checks
# `aapt2 dump badging` output. scripts/test-verify-android-apk.sh uses these
# so the checks stay covered without an SDK.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# apksigner spells the signer line two ways:
#   Signer #1 certificate DN:  (build-tools 36)
#   V2 Signer: certificate DN: (the copy on GitHub-hosted runners)
# Both lines contain "certificate DN:".
#
# Match with [[ ]] rather than `grep -q`. Under `set -o pipefail`, grep -q
# closes the pipe at the first hit and the writer dies with SIGPIPE, so a
# listing that does contain lib/arm64-v8a/ is reported as missing.
check_release_certs() {
  local certs="$1"
  if [[ "$certs" == *"CN=Android Debug"* ]]; then
    echo "release APK is signed with the Android debug key" >&2
    return 1
  fi
  if [[ "$certs" != *"certificate DN:"* ]]; then
    echo "release APK has no signer certificate" >&2
    printf '%s\n' "$certs" >&2
    return 1
  fi
}

check_release_listing() {
  local listing="$1"
  if [[ "$listing" != *"lib/arm64-v8a/"* ]]; then
    echo "release APK is missing lib/arm64-v8a" >&2
    return 1
  fi
  if [[ "$listing" =~ lib/(x86_64|x86|armeabi-v7a|armeabi)/ ]]; then
    echo "release APK contains an ABI other than arm64-v8a" >&2
    return 1
  fi
}

check_release_version() {
  local badging="$1"
  local want="$2"
  if [[ -z "$want" ]]; then
    echo "expected release version is empty" >&2
    return 1
  fi
  local want_code
  if ! want_code="$(bash "$SCRIPT_DIR/android-version-code.sh" "$want" 2>/dev/null)"; then
    echo "expected release version '$want' is not a valid version" >&2
    return 1
  fi
  local got_name got_code
  got_name="$(printf '%s\n' "$badging" |
    sed -n "s/^package:.*versionName='\([^']*\)'.*$/\1/p" |
    head -n 1)"
  if [[ "$got_name" != "$want" ]]; then
    echo "release APK versionName '$got_name' does not match expected '$want'" >&2
    return 1
  fi
  got_code="$(printf '%s\n' "$badging" |
    sed -n "s/^package:.*versionCode='\([^']*\)'.*$/\1/p" |
    head -n 1)"
  if [[ "$got_code" != "$want_code" ]]; then
    echo "release APK versionCode '$got_code' does not match expected '$want_code'" >&2
    return 1
  fi
}

if [[ "${1:-}" == "--release-certs-text" ]]; then
  check_release_certs "$(cat)"
  exit 0
fi

if [[ "${1:-}" == "--release-listing-text" ]]; then
  check_release_listing "$(cat)"
  exit 0
fi

if [[ "${1:-}" == "--release-badging-text" ]]; then
  check_release_version "$(cat)" "${2:-}"
  exit 0
fi

RELEASE=0
EXPECT_VERSION=""
while (($# > 0)); do
  case "$1" in
  --release)
    RELEASE=1
    shift
    ;;
  --version)
    EXPECT_VERSION="${2:-}"
    if [[ -z "$EXPECT_VERSION" ]]; then
      echo "--version needs a value" >&2
      exit 2
    fi
    shift 2
    ;;
  --*)
    echo "unknown option: $1" >&2
    exit 2
    ;;
  *)
    break
    ;;
  esac
done
APK="${1:-}"
if [[ -z "$APK" || ! -f "$APK" ]]; then
  echo "usage: $0 [--release] [--version <ver>] <apk>" >&2
  exit 1
fi

if [[ -z "${ANDROID_HOME:-}" ]]; then
  if [[ -d "$HOME/Android/Sdk" ]]; then
    ANDROID_HOME="$HOME/Android/Sdk"
  elif [[ -d "$HOME/Library/Android/sdk" ]]; then
    ANDROID_HOME="$HOME/Library/Android/sdk"
  fi
fi
if [[ -z "${ANDROID_HOME:-}" || ! -d "$ANDROID_HOME" ]]; then
  echo "Android SDK not found (set ANDROID_HOME)" >&2
  exit 1
fi
export ANDROID_HOME

find_build_tools() {
  local name="$1"
  local found
  found="$(find "$ANDROID_HOME/build-tools" -type f -name "$name")"
  if [[ -z "$found" ]]; then
    return 1
  fi
  # Version sort so build-tools 35 wins over 9 (plain sort would not).
  if sort -V </dev/null >/dev/null 2>&1; then
    printf '%s\n' "$found" | sort -V | tail -n 1
  else
    printf '%s\n' "$found" | sort | tail -n 1
  fi
}

APKSIGNER="$(find_build_tools apksigner || true)"
if [[ -z "$APKSIGNER" ]]; then
  echo "apksigner not found under $ANDROID_HOME/build-tools" >&2
  exit 1
fi

"$APKSIGNER" verify --verbose --print-certs "$APK"

if [[ "$RELEASE" == 1 ]]; then
  if ! certs="$("$APKSIGNER" verify --print-certs "$APK" 2>&1)"; then
    printf '%s\n' "$certs" >&2
    exit 1
  fi
  check_release_certs "$certs"
  check_release_listing "$(unzip -l "$APK")"
fi

if [[ -n "$EXPECT_VERSION" ]]; then
  if [[ -z "${ANDROID_HOME:-}" ]]; then
    echo "ANDROID_HOME is not set; cannot locate aapt/aapt2" >&2
    exit 1
  fi
  if [[ ! -d "$ANDROID_HOME/build-tools" ]]; then
    echo "ANDROID_HOME/build-tools not found: $ANDROID_HOME/build-tools" >&2
    exit 1
  fi
  AAPT="$(find_build_tools aapt2 || true)"
  if [[ -z "$AAPT" ]]; then
    AAPT="$(find_build_tools aapt || true)"
  fi
  if [[ -z "$AAPT" ]]; then
    echo "aapt/aapt2 not found under $ANDROID_HOME/build-tools" >&2
    exit 1
  fi
  check_release_version "$("$AAPT" dump badging "$APK")" "$EXPECT_VERSION"
fi

echo "verify-android-apk: OK ($APK)"
