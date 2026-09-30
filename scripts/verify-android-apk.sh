#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Check an ice-box APK signature.
# Usage: scripts/verify-android-apk.sh [--release] <apk>
#
# Without --release, the APK must verify (the CI debug APK is signed with
# the Android debug key). With --release, the signer must not be that debug
# key, and the native libraries must be arm64-v8a only.
#
# --release-certs-text reads `apksigner verify --print-certs` output on stdin
# and applies only the release signer checks. --release-listing-text does the
# same for `unzip -l` output. scripts/test-verify-android-apk.sh uses both so
# the checks stay covered without an SDK.
set -euo pipefail

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

if [[ "${1:-}" == "--release-certs-text" ]]; then
  check_release_certs "$(cat)"
  exit 0
fi

if [[ "${1:-}" == "--release-listing-text" ]]; then
  check_release_listing "$(cat)"
  exit 0
fi

RELEASE=0
if [[ "${1:-}" == "--release" ]]; then
  RELEASE=1
  shift
fi
APK="${1:-}"
if [[ -z "$APK" || ! -f "$APK" ]]; then
  echo "usage: $0 [--release] <apk>" >&2
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

APKSIGNER="$(find "$ANDROID_HOME/build-tools" -type f -name apksigner | sort | tail -n 1)"
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

echo "verify-android-apk: OK ($APK)"
