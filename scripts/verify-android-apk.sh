#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Check an ice-box APK signature.
# Usage: scripts/verify-android-apk.sh [--release] <apk>
#
# Without --release, the APK must verify (the CI debug APK is signed with
# the Android debug key). With --release, the signer must not be that debug
# key, and the native libraries must be arm64-v8a only.
set -euo pipefail

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
  certs="$("$APKSIGNER" verify --print-certs "$APK")"
  if printf '%s\n' "$certs" | grep -q "CN=Android Debug"; then
    echo "release APK is signed with the Android debug key" >&2
    exit 1
  fi
  if ! printf '%s\n' "$certs" | grep -q "Signer #1 certificate DN:"; then
    echo "release APK has no signer certificate" >&2
    exit 1
  fi
  listing="$(unzip -l "$APK")"
  if ! printf '%s\n' "$listing" | grep -q "lib/arm64-v8a/"; then
    echo "release APK is missing lib/arm64-v8a" >&2
    exit 1
  fi
  if printf '%s\n' "$listing" | grep -qE 'lib/(x86_64|x86|armeabi-v7a|armeabi)/'; then
    echo "release APK contains an ABI other than arm64-v8a" >&2
    exit 1
  fi
fi

echo "verify-android-apk: OK ($APK)"
