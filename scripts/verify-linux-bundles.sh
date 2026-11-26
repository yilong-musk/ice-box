#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Verify built Linux desktop bundles (.deb and .AppImage):
#   - canonical filenames (normalizing Tauri's occasional `_arm64.AppImage` on
#     aarch64 to `_aarch64.AppImage`, along with its `.sig` when present)
#   - required payload files inside BOTH `.deb` and extracted `.AppImage`
#     (`usr/bin/ice-box`, `usr/lib/ice-box/sing-box`, `usr/lib/ice-box/ice-helper`,
#     `usr/lib/ice-box/geoip/geoip-cn.srs`, `usr/lib/ice-box/geoip/geosite-cn.srs`,
#     plus executable `AppRun` inside `.AppImage`)
#   - glibc symbol ceiling (`<= 2.35` by default via scripts/check-glibc-floor.sh)
#     for the binaries inside BOTH `.deb` and `.AppImage`
#   - optional updater signature presence (--require-sig)
#
# Usage:
#   scripts/verify-linux-bundles.sh \
#     --bundle-dir <path> --deb-arch <amd64|arm64> --appimage-arch <amd64|aarch64> \
#     [--version <ver>] [--require-sig] [--glibc-max <ver>] [--glibc-script <path>]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUNDLE_DIR="target/release/bundle"
VERSION=""
DEB_ARCH=""
APPIMAGE_ARCH=""
REQUIRE_SIG=0
GLIBC_MAX="2.35"
GLIBC_SCRIPT="$ROOT/scripts/check-glibc-floor.sh"

usage() {
  cat >&2 <<'EOF'
usage: scripts/verify-linux-bundles.sh \
  --deb-arch <amd64|arm64> --appimage-arch <amd64|aarch64> \
  [--bundle-dir <path>] [--version <ver>] [--require-sig] \
  [--glibc-max <ver>] [--glibc-script <path>]
EOF
}

while (($# > 0)); do
  case "$1" in
  --bundle-dir)
    BUNDLE_DIR="${2:-}"
    shift 2
    ;;
  --version)
    VERSION="${2:-}"
    shift 2
    ;;
  --deb-arch)
    DEB_ARCH="${2:-}"
    shift 2
    ;;
  --appimage-arch)
    APPIMAGE_ARCH="${2:-}"
    shift 2
    ;;
  --require-sig)
    REQUIRE_SIG=1
    shift
    ;;
  --glibc-max)
    GLIBC_MAX="${2:-}"
    shift 2
    ;;
  --glibc-script)
    GLIBC_SCRIPT="${2:-}"
    shift 2
    ;;
  -h | --help)
    usage
    exit 0
    ;;
  *)
    echo "verify-linux-bundles: unknown argument: $1" >&2
    usage
    exit 2
    ;;
  esac
done

if [[ -z "$DEB_ARCH" || -z "$APPIMAGE_ARCH" ]]; then
  echo "verify-linux-bundles: --deb-arch and --appimage-arch are required" >&2
  usage
  exit 2
fi

if [[ -z "$VERSION" ]]; then
  VERSION="$(python3 -c 'import json, sys; print(json.load(open(sys.argv[1]))["version"])' "$ROOT/apps/desktop/package.json")"
fi
VERSION="${VERSION#v}"
if [[ -z "$VERSION" ]]; then
  echo "verify-linux-bundles: could not resolve version" >&2
  exit 1
fi

shopt -s nullglob

debs=("$BUNDLE_DIR/deb"/*.deb)
if [[ "${#debs[@]}" -ne 1 ]]; then
  echo "verify-linux-bundles: expected exactly 1 .deb under $BUNDLE_DIR/deb, found ${#debs[@]}" >&2
  exit 1
fi
expected_deb="ice-box_${VERSION}_${DEB_ARCH}.deb"
if [[ "$(basename "${debs[0]}")" != "$expected_deb" ]]; then
  echo "verify-linux-bundles: unexpected .deb artifact name: ${debs[0]} (expected $expected_deb)" >&2
  exit 1
fi
if [[ ! -s "${debs[0]}" ]]; then
  echo "verify-linux-bundles: empty .deb artifact: ${debs[0]}" >&2
  exit 1
fi

REQUIRED_FILES=(
  usr/bin/ice-box
  usr/lib/ice-box/sing-box
  usr/lib/ice-box/ice-helper
  usr/lib/ice-box/geoip/geoip-cn.srs
  usr/lib/ice-box/geoip/geosite-cn.srs
)
REQUIRED_BINS=(
  usr/bin/ice-box
  usr/lib/ice-box/ice-helper
  usr/lib/ice-box/sing-box
)

listing="$(dpkg-deb -c "${debs[0]}")"
echo "$listing"
for rel in "${REQUIRED_FILES[@]}"; do
  regex="${rel//./\\.}"
  echo "$listing" | grep -Eq "^-.*[[:space:]](\./|/)?${regex}$" || {
    echo "verify-linux-bundles: missing /${rel} in .deb payload (${debs[0]})" >&2
    exit 1
  }
done

WORK="$(mktemp -d "${TMPDIR:-/tmp}/ice-box-verify-linux-XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

deb_extract="$WORK/deb"
mkdir -p "$deb_extract"
dpkg-deb -x "${debs[0]}" "$deb_extract"
for rel in "${REQUIRED_BINS[@]}"; do
  if [[ ! -x "$deb_extract/$rel" ]]; then
    echo "verify-linux-bundles: missing or non-executable /${rel} in extracted .deb (${debs[0]})" >&2
    exit 1
  fi
done
bash "$GLIBC_SCRIPT" "$GLIBC_MAX" \
  "$deb_extract/usr/bin/ice-box" \
  "$deb_extract/usr/lib/ice-box/ice-helper" \
  "$deb_extract/usr/lib/ice-box/sing-box"

appimages=("$BUNDLE_DIR/appimage"/*.AppImage)
if [[ "${#appimages[@]}" -ne 1 ]]; then
  echo "verify-linux-bundles: expected exactly 1 .AppImage under $BUNDLE_DIR/appimage, found ${#appimages[@]}" >&2
  exit 1
fi

sigs=("$BUNDLE_DIR/appimage"/*.AppImage.sig)
if [[ "$REQUIRE_SIG" -eq 1 ]]; then
  if [[ "${#sigs[@]}" -ne 1 ]]; then
    echo "verify-linux-bundles: expected exactly 1 .AppImage.sig under $BUNDLE_DIR/appimage, found ${#sigs[@]}" >&2
    exit 1
  fi
  if [[ ! -s "${sigs[0]}" ]]; then
    echo "verify-linux-bundles: empty updater signature: ${sigs[0]}" >&2
    exit 1
  fi
fi

expected_appimage="ice-box_${VERSION}_${APPIMAGE_ARCH}.AppImage"
actual_appimage="$(basename "${appimages[0]}")"
appimage_path="$BUNDLE_DIR/appimage/${expected_appimage}"
if [[ "$actual_appimage" != "$expected_appimage" ]]; then
  # Some Tauri bundler versions emit `_arm64.AppImage` on aarch64. Normalize to
  # the canonical `_aarch64.AppImage` name; minisign signs file bytes rather
  # than the filename.
  if [[ "$APPIMAGE_ARCH" == "aarch64" && "$actual_appimage" == "ice-box_${VERSION}_arm64.AppImage" ]]; then
    mv "${appimages[0]}" "$appimage_path"
    if [[ -f "${appimages[0]}.sig" ]]; then
      mv "${appimages[0]}.sig" "${appimage_path}.sig"
    fi
  else
    echo "verify-linux-bundles: unexpected AppImage artifact name: ${appimages[0]} (expected $expected_appimage)" >&2
    exit 1
  fi
fi

if [[ ! -s "$appimage_path" ]]; then
  echo "verify-linux-bundles: empty AppImage artifact: $appimage_path" >&2
  exit 1
fi
if [[ "$REQUIRE_SIG" -eq 1 && ! -s "${appimage_path}.sig" ]]; then
  echo "verify-linux-bundles: missing updater signature: ${appimage_path}.sig" >&2
  exit 1
fi

appimage_abs="$(cd "$(dirname "$appimage_path")" && pwd)/$(basename "$appimage_path")"
chmod +x "$appimage_abs"

appimage_extract="$WORK/appimage"
mkdir -p "$appimage_extract"
(
  cd "$appimage_extract"
  APPIMAGE_EXTRACT_AND_RUN=1 "$appimage_abs" --appimage-extract >/dev/null
)

appimage_root="$appimage_extract/squashfs-root"
if [[ ! -d "$appimage_root" ]]; then
  echo "verify-linux-bundles: AppImage extraction did not produce squashfs-root ($appimage_path)" >&2
  exit 1
fi
if [[ ! -x "$appimage_root/AppRun" ]]; then
  echo "verify-linux-bundles: missing executable AppRun in AppImage ($appimage_path)" >&2
  exit 1
fi
for rel in "${REQUIRED_FILES[@]}"; do
  if [[ ! -s "$appimage_root/$rel" ]]; then
    echo "verify-linux-bundles: missing /${rel} in AppImage payload ($appimage_path)" >&2
    exit 1
  fi
done
for rel in "${REQUIRED_BINS[@]}"; do
  if [[ ! -x "$appimage_root/$rel" ]]; then
    echo "verify-linux-bundles: non-executable /${rel} in AppImage payload ($appimage_path)" >&2
    exit 1
  fi
done

bash "$GLIBC_SCRIPT" "$GLIBC_MAX" \
  "$appimage_root/usr/bin/ice-box" \
  "$appimage_root/usr/lib/ice-box/ice-helper" \
  "$appimage_root/usr/lib/ice-box/sing-box"

echo "verified .deb: ${debs[0]}"
echo "verified AppImage: ${appimage_path}"
if [[ "$REQUIRE_SIG" -eq 1 ]]; then
  echo "verified updater signature: ${appimage_path}.sig"
fi
