#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Build an Android APK and verify its signature.
# Usage: scripts/build-android-apk.sh debug|release [arm64|amd64]
#
# debug is signed with the Android debug key. release requires:
#   ICE_BOX_ANDROID_KEYSTORE              path outside this repository
#   ICE_BOX_ANDROID_KEYSTORE_PASSWORD
#   ICE_BOX_ANDROID_KEY_ALIAS
#   ICE_BOX_ANDROID_KEY_PASSWORD
#
# ICE_BOX_SKIP_LIBBOX=1 reuses apps/mobile/.../libs/libbox.aar when it exists.
# arm64 is the published ABI. amd64 is the emulator.
# GeoIP rule-sets are fetched and packaged as APK assets. The phone copies
# them into its files directory before it writes config.json.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MODE="${1:-}"
ABI="${2:-arm64}"

case "$MODE" in
debug | release) ;;
*)
  echo "usage: $0 debug|release [arm64|amd64]" >&2
  exit 1
  ;;
esac

case "$ABI" in
arm64) TAURI_TARGET="aarch64" ;;
amd64) TAURI_TARGET="x86_64" ;;
*)
  echo "unknown ABI '$ABI' (expected arm64 or amd64)" >&2
  exit 1
  ;;
esac

bash "$ROOT/scripts/fetch-geoip.sh"

AAR="$ROOT/apps/mobile/src-tauri/gen/android/app/libs/libbox.aar"
if [[ "${ICE_BOX_SKIP_LIBBOX:-}" == 1 && -f "$AAR" ]]; then
  echo "using existing $AAR"
else
  bash "$ROOT/scripts/build-libbox.sh" android "$ABI"
fi

if [[ "$MODE" == release ]]; then
  : "${ICE_BOX_ANDROID_KEYSTORE:?set ICE_BOX_ANDROID_KEYSTORE to the release keystore}"
  : "${ICE_BOX_ANDROID_KEYSTORE_PASSWORD:?set ICE_BOX_ANDROID_KEYSTORE_PASSWORD}"
  : "${ICE_BOX_ANDROID_KEY_ALIAS:?set ICE_BOX_ANDROID_KEY_ALIAS}"
  : "${ICE_BOX_ANDROID_KEY_PASSWORD:?set ICE_BOX_ANDROID_KEY_PASSWORD}"
  if [[ ! -f "$ICE_BOX_ANDROID_KEYSTORE" ]]; then
    echo "keystore not found: $ICE_BOX_ANDROID_KEYSTORE" >&2
    exit 1
  fi
  dest="$(realpath "$ICE_BOX_ANDROID_KEYSTORE")"
  root="$(realpath "$ROOT")"
  case "$dest" in
  "$root" | "$root"/*)
    echo "refusing to sign with a keystore inside the repository: $dest" >&2
    exit 1
    ;;
  esac
fi

cd "$ROOT/apps/mobile"
args=(android build --target "$TAURI_TARGET" --apk --ci)
if [[ "$MODE" == debug ]]; then
  args+=(--debug)
fi
CI=1 npm run tauri -- "${args[@]}"

apk_dir="$ROOT/apps/mobile/src-tauri/gen/android/app/build/outputs/apk"
apk="$(
  find "$apk_dir" -type f -path "*/${MODE}/*.apk" -printf '%T@ %p\n' |
    sort -nr |
    awk 'NR == 1 { print $2 }'
)"
if [[ -z "$apk" || ! -f "$apk" ]]; then
  echo "no ${MODE} APK under $apk_dir" >&2
  exit 1
fi

verify_args=()
if [[ "$MODE" == release ]]; then
  verify_args+=(--release)
fi
bash "$ROOT/scripts/verify-android-apk.sh" "${verify_args[@]}" "$apk"
echo "build-android-apk: $apk"
