#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Build the libbox AAR from the pinned sing-box tag.
# Usage: scripts/build-libbox.sh android [arm64|amd64|all]
#
# arm64 is the device ABI. amd64 is the emulator. all builds arm64 and amd64.
# The core version is third_party/sing-box/VERSION. The build itself is that
# tag's cmd/internal/build_libbox (JDK, NDK, API level, ldflags, tag list).
# This script only narrows the result: skip a legacy AAR when the tag still
# builds one, and keep the feature tags config generation emits plus upstream
# toolchain tags. A newer sing-box release is picked up by editing VERSION and
# CHECKSUMS.sha256; add a feature tag here only when generation starts emitting
# that protocol.
#
# The embedded AAR version is the git tag, which must equal VERSION
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
  echo "Set JAVA_HOME to a JDK. build_libbox checks the major version for this sing-box tag." >&2
  exit 1
fi
export JAVA_HOME

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
if [[ ! -f "$ANDROID_HOME/licenses/android-sdk-license" ]]; then
  echo "Android SDK licenses not accepted ($ANDROID_HOME/licenses/android-sdk-license is missing)" >&2
  exit 1
fi
if [[ ! -d "$ANDROID_HOME/ndk" ]]; then
  echo "Android NDK not found under $ANDROID_HOME/ndk" >&2
  exit 1
fi
# build_libbox selects NDK 28.0.13004108 when it is installed, and otherwise
# falls back. Leave ANDROID_NDK_HOME unset unless the caller set it, so that
# selection stays inside the upstream tool.
export PATH="$JAVA_HOME/bin:${PATH}"

if ! command -v go >/dev/null 2>&1; then
  echo "Go is required to build libbox" >&2
  exit 1
fi
if ! command -v python3 >/dev/null 2>&1; then
  echo "python3 is required to build libbox" >&2
  exit 1
fi

CACHE="${LIBBOX_SRC:-$HOME/.cache/ice-box/sing-box-$VERSION}"
if [[ ! -d "$CACHE/.git" ]]; then
  mkdir -p "$(dirname "$CACHE")"
  echo "Cloning sing-box v$VERSION"
  git clone --depth 1 --branch "v$VERSION" https://github.com/SagerNet/sing-box.git "$CACHE"
fi
# build_libbox embeds `git describe --tags` with the leading "v" removed.
# Check out the tag itself so a dirty or moved cache cannot change the version.
if ! git -C "$CACHE" rev-parse --verify --quiet "refs/tags/v$VERSION" >/dev/null; then
  git -C "$CACHE" fetch --depth 1 origin tag "v$VERSION"
fi
git -C "$CACHE" checkout --detach --force --quiet "v$VERSION"
DESCRIBE="$(git -C "$CACHE" describe --tags)"
if [[ "$DESCRIBE" != "v$VERSION" ]]; then
  echo "libbox source describe '$DESCRIBE' does not match pinned v$VERSION" >&2
  exit 1
fi

GOMOBILE_VERSION="$(awk '$1 == "github.com/sagernet/gomobile" { print $2; exit }' "$CACHE/go.mod")"
if [[ -z "$GOMOBILE_VERSION" ]]; then
  echo "sing-box go.mod has no github.com/sagernet/gomobile requirement" >&2
  exit 1
fi
# build_libbox looks up gobind in GOPATH/bin, ignoring GOBIN.
GOPATH_BIN="$(go env GOPATH)/bin"
export PATH="$GOPATH_BIN:$PATH"
GOMOBILE_STAMP="$GOPATH_BIN/.ice-box-gomobile-version"
if [[ "$(cat "$GOMOBILE_STAMP" 2>/dev/null || true)" != "$GOMOBILE_VERSION" || ! -x "$GOPATH_BIN/gomobile" || ! -x "$GOPATH_BIN/gobind" ]]; then
  echo "Installing SagerNet gomobile $GOMOBILE_VERSION"
  env -u GOBIN go install -v "github.com/sagernet/gomobile/cmd/gomobile@${GOMOBILE_VERSION}"
  env -u GOBIN go install -v "github.com/sagernet/gomobile/cmd/gobind@${GOMOBILE_VERSION}"
  printf '%s\n' "$GOMOBILE_VERSION" >"$GOMOBILE_STAMP"
fi

OUT_DIR="$ROOT/apps/mobile/src-tauri/gen/android/app/libs"
mkdir -p "$OUT_DIR"
AAR="$OUT_DIR/libbox.aar"
LIBBOX_MAIN="$CACHE/cmd/internal/build_libbox/main.go"
if [[ ! -f "$LIBBOX_MAIN" ]]; then
  echo "missing $LIBBOX_MAIN" >&2
  exit 1
fi

# The cache stays on the upstream file. The edit exists only for this run.
restore_libbox_source() {
  local status=$?
  git -C "$CACHE" checkout --quiet -- cmd/internal/build_libbox/main.go || true
  exit "$status"
}
trap restore_libbox_source EXIT

LIBBOX_MAIN="$LIBBOX_MAIN" python3 - <<'PY'
import os
import re
import sys
from pathlib import Path

src = Path(os.environ["LIBBOX_MAIN"])
text = src.read_text()

# Feature tags config generation emits today. Other with_* tags (naive,
# tailscale, and protocols added in a later sing-box release) are dropped
# until generation learns them. Tags without the with_ prefix are toolchain
# tags and stay as upstream wrote them. ts_omit_* only applies to Tailscale.
keep_with = {
    "with_gvisor",
    "with_quic",
    "with_wireguard",
    "with_utls",
    "with_clash_api",
}

def keep(tag: str) -> bool:
    if tag.startswith("ts_omit_"):
        return False
    if tag.startswith("with_"):
        return tag in keep_with
    return True

kept = []
dropped = []

def rewrite_tags(match):
    line = match.group(0)
    tags = re.findall(r'"([^"]*)"', line)
    kept_here = []
    for tag in tags:
        if keep(tag):
            kept_here.append(tag)
            kept.append(tag)
        else:
            dropped.append(tag)
    if not kept_here:
        return ""
    indent = re.match(r"[ \t]*", line).group(0)
    joined = ", ".join(f'"{tag}"' for tag in kept_here)
    return f"{indent}sharedTags = append(sharedTags, {joined})\n"

text, replacements = re.subn(
    r"^[ \t]*sharedTags = append\(sharedTags,.*?\)\n",
    rewrite_tags,
    text,
    flags=re.M | re.S,
)
if replacements < 1:
    sys.exit(
        "build_libbox: no sharedTags append found. "
        "Update scripts/build-libbox.sh for this sing-box release."
    )
missing = sorted(keep_with - set(kept))
if missing:
    sys.exit(
        "build_libbox is missing feature tags config generation needs: "
        + ", ".join(missing)
        + ". Update the allowlist and the config generator together."
    )

def code_of(line: str) -> str:
    return line.split("//", 1)[0]

def has_legacy(source: str) -> bool:
    return any("libbox-legacy.aar" in code_of(line) for line in source.splitlines())

lines = text.splitlines(keepends=True)
out = []
index = 0
removed = False
if has_legacy(text):
    while index < len(lines):
        if "Build legacy variant" in lines[index]:
            index += 1
            started = False
            depth = 0
            while index < len(lines):
                line = lines[index]
                index += 1
                if "buildAndroidVariant(" in line:
                    started = True
                if not started:
                    continue
                depth += line.count("(") - line.count(")")
                if depth <= 0:
                    break
            removed = True
            continue
        out.append(lines[index])
        index += 1
    if not removed:
        sys.exit(
            "build_libbox: this release still builds libbox-legacy.aar, but the "
            "legacy block was not recognized. Update scripts/build-libbox.sh "
            "before using this sing-box release."
        )
else:
    out = lines
    print("libbox: this release has no legacy AAR")

patched = "".join(out)
if has_legacy(patched):
    sys.exit("build_libbox: libbox-legacy.aar is still built after the edit")
outputs = re.findall(r'OutputName:\s*"([^"]+)"', patched)
if outputs:
    extras = [name for name in outputs if name != "libbox.aar"]
    if "libbox.aar" not in outputs or extras:
        sys.exit(
            "build_libbox: expected only libbox.aar, found "
            + ", ".join(outputs)
            + ". Update scripts/build-libbox.sh for this sing-box release."
        )
elif "libbox.aar" not in patched:
    sys.exit(
        "build_libbox: primary libbox.aar output not found. "
        "Update scripts/build-libbox.sh for this sing-box release."
    )
for line in patched.splitlines():
    code = code_of(line)
    for tag in dropped:
        if f'"{tag}"' in code:
            sys.exit(f"build_libbox: dropped tag {tag} is still quoted in the builder")

src.write_text(patched)
print("libbox tags: " + ",".join(kept))
if dropped:
    print("libbox dropped tags: " + ",".join(dropped))
PY

echo "Building libbox $VERSION ($BIND_TARGET) with cmd/internal/build_libbox"
(
  cd "$CACHE"
  go run ./cmd/internal/build_libbox -target android -platform "$BIND_TARGET"
)
mv "$CACHE/libbox.aar" "$AAR"
rm -f "$CACHE/libbox-sources.jar"

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
