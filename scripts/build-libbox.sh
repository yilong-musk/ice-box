#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Build the libbox AAR from the pinned sing-box tag.
# Usage: scripts/build-libbox.sh android [arm64|amd64|all]
#
# arm64 is the device ABI. amd64 is the emulator. all builds both.
# The AAR version is the git tag, which must equal third_party/sing-box/VERSION
# (ENGINE_COMPAT_CORE_VERSION). Artifacts are not committed.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET="${1:-}"
ABI="${2:-arm64}"

if [[ "$TARGET" != "android" ]]; then
  echo "usage: scripts/build-libbox.sh android [arm64|amd64|all]" >&2
  exit 1
fi

VERSION="$(tr -d '[:space:]' <"$ROOT/third_party/sing-box/VERSION")"
if [[ -z "$VERSION" ]]; then
  echo "empty sing-box version pin" >&2
  exit 1
fi

case "$ABI" in
  arm64) BIND_TARGET="android/arm64" ;;
  amd64) BIND_TARGET="android/amd64" ;;
  all) BIND_TARGET="android/arm64,android/amd64" ;;
  *)
    echo "unknown ABI '$ABI' (expected arm64, amd64, or all)" >&2
    exit 1
    ;;
esac

if [[ -z "${JAVA_HOME:-}" ]]; then
  if [[ -d "/opt/homebrew/opt/openjdk@17" ]]; then
    JAVA_HOME="/opt/homebrew/opt/openjdk@17"
  elif [[ -d "/usr/lib/jvm/java-17-openjdk-amd64" ]]; then
    JAVA_HOME="/usr/lib/jvm/java-17-openjdk-amd64"
  fi
fi
if [[ -z "${JAVA_HOME:-}" || ! -x "$JAVA_HOME/bin/java" ]]; then
  echo "OpenJDK 17 is required (set JAVA_HOME)" >&2
  exit 1
fi
export JAVA_HOME
JAVA_VERSION="$("$JAVA_HOME/bin/java" --version 2>&1 || true)"
if [[ "$JAVA_VERSION" != *"openjdk 17"* ]]; then
  echo "java version should be openjdk 17" >&2
  echo "$JAVA_VERSION" >&2
  exit 1
fi

if [[ -z "${ANDROID_HOME:-}" ]]; then
  if [[ -d "$HOME/Library/Android/sdk" ]]; then
    ANDROID_HOME="$HOME/Library/Android/sdk"
  elif [[ -d "$HOME/Android/Sdk" ]]; then
    ANDROID_HOME="$HOME/Android/Sdk"
  fi
fi
if [[ -z "${ANDROID_HOME:-}" || ! -d "$ANDROID_HOME" ]]; then
  echo "Android SDK not found (set ANDROID_HOME)" >&2
  exit 1
fi
export ANDROID_HOME
export ANDROID_SDK_HOME="$ANDROID_HOME"

NDK_PIN="28.0.13004108"
if [[ -z "${ANDROID_NDK_HOME:-}" ]]; then
  if [[ -d "$ANDROID_HOME/ndk/$NDK_PIN" ]]; then
    ANDROID_NDK_HOME="$ANDROID_HOME/ndk/$NDK_PIN"
  else
    ANDROID_NDK_HOME="$(find "$ANDROID_HOME/ndk" -mindepth 1 -maxdepth 1 -type d | sort -V | tail -n 1)"
  fi
fi
if [[ -z "${ANDROID_NDK_HOME:-}" || ! -f "$ANDROID_NDK_HOME/source.properties" ]]; then
  echo "Android NDK not found under $ANDROID_HOME/ndk" >&2
  exit 1
fi
export ANDROID_NDK_HOME
export NDK="$ANDROID_NDK_HOME"

HOST_PREBUILT=""
for candidate in "$ANDROID_NDK_HOME/toolchains/llvm/prebuilt"/*; do
  if [[ -d "$candidate" ]]; then
    HOST_PREBUILT="$candidate"
    break
  fi
done
if [[ -z "$HOST_PREBUILT" ]]; then
  echo "NDK LLVM prebuilt toolchain not found" >&2
  exit 1
fi
export PATH="$JAVA_HOME/bin:$HOST_PREBUILT/bin:${PATH}"

if ! command -v go >/dev/null 2>&1; then
  echo "Go is required to build libbox" >&2
  exit 1
fi

GOBIN="${GOBIN:-$(go env GOPATH)/bin}"
export PATH="$GOBIN:$PATH"
GOMOBILE_MODULE="github.com/sagernet/gomobile"
GOMOBILE_VERSION="v0.1.12"
if [[ ! -x "$GOBIN/gomobile" || ! -x "$GOBIN/gobind" ]]; then
  echo "Installing SagerNet gomobile $GOMOBILE_VERSION"
  go install -v "$GOMOBILE_MODULE/cmd/gomobile@$GOMOBILE_VERSION"
  go install -v "$GOMOBILE_MODULE/cmd/gobind@$GOMOBILE_VERSION"
fi

CACHE="${LIBBOX_SRC:-$HOME/.cache/ice-box/sing-box-$VERSION}"
if [[ ! -d "$CACHE/.git" ]]; then
  mkdir -p "$(dirname "$CACHE")"
  echo "Cloning sing-box v$VERSION"
  git clone --depth 1 --branch "v$VERSION" https://github.com/SagerNet/sing-box.git "$CACHE"
fi

TAG="$(git -C "$CACHE" describe --tags --abbrev=0)"
if [[ "$TAG" != "v$VERSION" ]]; then
  echo "libbox source tag $TAG does not match pinned v$VERSION" >&2
  exit 1
fi

# Flags match sing-box v1.13.19 cmd/internal/build_libbox for the API 23 AAR.
# The legacy API 21 AAR is not built: this client requires Android 14.
TAGS="with_gvisor,with_quic,with_wireguard,with_utls,with_naive_outbound,with_clash_api,badlinkname,tfogo_checklinkname0,with_tailscale,ts_omit_logtail,ts_omit_ssh,ts_omit_drive,ts_omit_taildrop,ts_omit_webclient,ts_omit_doctor,ts_omit_capture,ts_omit_kube,ts_omit_aws,ts_omit_synology,ts_omit_bird"
LDFLAGS="-X github.com/sagernet/sing-box/constant.Version=$VERSION -X internal/godebug.defaultGODEBUG=multipathtcp=0 -s -w -buildid= -checklinkname=0"

OUT_DIR="$ROOT/apps/mobile/src-tauri/gen/android/app/libs"
mkdir -p "$OUT_DIR"
AAR="$OUT_DIR/libbox.aar"

echo "Building libbox $VERSION ($BIND_TARGET) with NDK $(basename "$ANDROID_NDK_HOME")"
(
  cd "$CACHE"
  gomobile bind -v \
    -o "$AAR" \
    -target "$BIND_TARGET" \
    -androidapi 23 \
    -javapkg=io.nekohasekai \
    -libname=box \
    -trimpath \
    -buildvcs=false \
    -ldflags "$LDFLAGS" \
    -tags "$TAGS" \
    ./experimental/libbox
)

if [[ ! -f "$AAR" ]]; then
  echo "libbox AAR was not produced" >&2
  exit 1
fi
# gomobile also writes a sources jar beside the AAR. It is not packaged.
rm -f "${AAR%.aar}-sources.jar"

if command -v shasum >/dev/null 2>&1; then
  SHA256_CMD="shasum -a 256"
elif command -v sha256sum >/dev/null 2>&1; then
  SHA256_CMD="sha256sum"
else
  echo "no SHA-256 tool (shasum/sha256sum) available" >&2
  exit 1
fi
SUM="$($SHA256_CMD "$AAR")"
printf '%s\n' "$SUM" >"$ROOT/third_party/sing-box/libbox-android.sha256"
echo "SHA-256 $SUM"
echo "Wrote $AAR"
