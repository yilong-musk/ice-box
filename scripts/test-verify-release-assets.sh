#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Fixture checks for scripts/verify-release-assets.sh.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SCRIPT="$ROOT/scripts/verify-release-assets.sh"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/ice-box-verify-release-XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

fail() {
  echo "test-verify-release-assets: $*" >&2
  exit 1
}

# Fake gh: the script only calls `gh release view` and
# `gh api repos/<repo>/releases/latest`.
mkdir -p "$TMP/bin"
cat >"$TMP/bin/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
case "$1" in
release)
  cat "$FIXTURE_META"
  ;;
api)
  if [[ -z "${FIXTURE_LATEST_TAG:-}" ]]; then
    echo "release not found" >&2
    exit 1
  fi
  printf '%s\n' "$FIXTURE_LATEST_TAG"
  ;;
*)
  echo "unexpected gh arguments: $*" >&2
  exit 2
  ;;
esac
EOF
chmod +x "$TMP/bin/gh"

build_fixture() {
  local dir="$1"
  local version="$2"
  local include_linux="$3"
  local include_latest="$4"
  local meta_out="$5"
  local prerelease_flag="$6"

  mkdir -p "$dir"
  printf 'dmg\n' >"$dir/ice-box_${version}_aarch64.dmg"
  printf 'tar\n' >"$dir/ice-box.app.tar.gz"
  printf 'sig\n' >"$dir/ice-box.app.tar.gz.sig"
  printf 'exe\n' >"$dir/ice-box_${version}_x64-setup.exe"
  printf 'sig\n' >"$dir/ice-box_${version}_x64-setup.exe.sig"
  if [[ "$include_linux" == "1" ]]; then
    printf 'deb-x64\n' >"$dir/ice-box_${version}_amd64.deb"
    printf 'appimage-x64\n' >"$dir/ice-box_${version}_amd64.AppImage"
    printf 'sig\n' >"$dir/ice-box_${version}_amd64.AppImage.sig"
    printf 'deb-arm64\n' >"$dir/ice-box_${version}_arm64.deb"
    printf 'appimage-arm64\n' >"$dir/ice-box_${version}_aarch64.AppImage"
    printf 'sig\n' >"$dir/ice-box_${version}_aarch64.AppImage.sig"
  fi
  printf 'apk\n' >"$dir/ice-box_${version}_android_arm64.apk"
  printf 'license\n' >"$dir/LICENSE"
  printf 'notice\n' >"$dir/NOTICE"
  printf 'sing-box license\n' >"$dir/sing-box-LICENSE"

  python3 - "$dir" "$version" "$include_linux" "$include_latest" "$meta_out" "$prerelease_flag" <<'PY'
import hashlib
import json
import sys
from pathlib import Path

root = Path(sys.argv[1])
version = sys.argv[2]
include_linux = sys.argv[3] == "1"
include_latest = sys.argv[4] == "1"
meta_out = Path(sys.argv[5])
prerelease = sys.argv[6] == "1"
tag = f"v{version}"

if include_latest:
    platforms = {
        "darwin-aarch64": {
            "signature": "sig",
            "url": f"https://github.com/fixture/repo/releases/download/{tag}/ice-box.app.tar.gz",
        },
        "windows-x86_64": {
            "signature": "sig",
            "url": f"https://github.com/fixture/repo/releases/download/{tag}/ice-box_{version}_x64-setup.exe",
        },
    }
    if include_linux:
        platforms["linux-x86_64"] = {
            "signature": "sig",
            "url": f"https://github.com/fixture/repo/releases/download/{tag}/ice-box_{version}_amd64.AppImage",
        }
        platforms["linux-aarch64"] = {
            "signature": "sig",
            "url": f"https://github.com/fixture/repo/releases/download/{tag}/ice-box_{version}_aarch64.AppImage",
        }
    (root / "latest.json").write_text(
        json.dumps({"version": version, "platforms": platforms}, indent=2) + "\n",
        encoding="utf-8",
    )

entries = []
for path in sorted(root.iterdir()):
    if path.name == "SHA256SUMS":
        continue
    entries.append((path.name, hashlib.sha256(path.read_bytes()).hexdigest()))
(root / "SHA256SUMS").write_text(
    "".join(f"{digest}  {name}\n" for name, digest in entries), encoding="utf-8"
)

names = [name for name, _ in entries] + ["SHA256SUMS"]
meta_out.write_text(
    json.dumps(
        {
            "isDraft": False,
            "isPrerelease": prerelease,
            "assets": [{"name": name} for name in names],
        }
    ),
    encoding="utf-8",
)
PY
}

# Linux-era final and pre-release fixtures (version > 0.4.0).
RELEASE="$TMP/release-0.5.0"
build_fixture "$RELEASE" "0.5.0" 1 1 "$TMP/meta-final.json" 0
build_fixture "$TMP/prerelease-0.5.0" "0.5.0" 1 0 "$TMP/meta-prerelease.json" 1
build_fixture "$TMP/prerelease-0.4.1-rc.1" "0.4.1-rc.1" 1 0 "$TMP/meta-0.4.1-rc.1.json" 1

# Linux-era release missing Linux assets (must fail).
build_fixture "$TMP/release-0.5.0-no-linux" "0.5.0" 0 1 "$TMP/meta-0.5.0-no-linux.json" 0
build_fixture "$TMP/prerelease-0.4.1-rc.1-no-linux" "0.4.1-rc.1" 0 0 "$TMP/meta-0.4.1-rc.1-no-linux.json" 1

# Pre-Linux fixtures (version <= 0.4.0): no Linux assets, macOS+Windows latest.json.
build_fixture "$TMP/release-0.4.0" "0.4.0" 0 1 "$TMP/meta-0.4.0.json" 0
build_fixture "$TMP/prerelease-0.4.0-rc.1" "0.4.0-rc.1" 0 0 "$TMP/meta-0.4.0-rc.1.json" 1
build_fixture "$TMP/release-0.4.0-with-linux" "0.4.0" 1 1 "$TMP/meta-0.4.0-with-linux.json" 0

python3 - "$TMP/meta-final.json" "$TMP/meta-prerelease-with-latest.json" \
  "$TMP/meta-extra.json" <<'PY'
import json
import sys
from pathlib import Path

base = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
Path(sys.argv[2]).write_text(
    json.dumps({**base, "isPrerelease": True}), encoding="utf-8"
)
Path(sys.argv[3]).write_text(
    json.dumps({**base, "assets": base["assets"] + [{"name": "extra.txt"}]}),
    encoding="utf-8",
)
PY

to_uri() {
  local path="$1"
  # Git Bash needs a native path before Python can turn it into a file URI.
  if command -v cygpath >/dev/null 2>&1; then
    path="$(cygpath -m "$path")"
  fi
  python3 -c 'import pathlib, sys; print(pathlib.Path(sys.argv[1]).resolve().as_uri())' "$path"
}

verify() {
  local base="$1"
  local meta="$2"
  local latest="$3"
  shift 3
  local base_uri
  base_uri="$(to_uri "$base")"
  PATH="$TMP/bin:$PATH" \
    GITHUB_REPOSITORY=fixture/repo \
    ICE_BOX_RELEASE_RETRY_ATTEMPTS=2 \
    ICE_BOX_RELEASE_RETRY_DELAY=0 \
    ICE_BOX_RELEASE_BASE_URL="$base_uri" \
    ICE_BOX_RELEASE_LATEST_URL="$base_uri/latest.json" \
    FIXTURE_META="$meta" \
    FIXTURE_LATEST_TAG="$latest" \
    bash "$SCRIPT" "$@"
}

expect_ok() {
  local name="$1"
  shift
  if ! "$@" >/dev/null 2>&1; then
    echo "test-verify-release-assets: $name: expected success" >&2
    "$@" >&2 || true
    exit 1
  fi
}

expect_fail() {
  local name="$1"
  local needle="$2"
  shift 2
  local err
  if err="$("$@" 2>&1)"; then
    echo "test-verify-release-assets: $name: expected failure" >&2
    exit 1
  fi
  if ! printf '%s\n' "$err" | grep -q "$needle"; then
    echo "test-verify-release-assets: $name: missing '$needle' in:" >&2
    printf '%s\n' "$err" >&2
    exit 1
  fi
}

echo "== happy paths (Linux-era > 0.4.0) =="
expect_ok "latest" verify "$RELEASE" "$TMP/meta-final.json" v0.5.0 v0.5.0
expect_ok "older-tag" verify "$RELEASE" "$TMP/meta-final.json" v0.5.1 v0.5.0
expect_ok "no-latest-release" verify "$RELEASE" "$TMP/meta-final.json" "" v0.5.0
expect_ok "prerelease" verify "$TMP/prerelease-0.5.0" "$TMP/meta-prerelease.json" v0.5.1 --prerelease v0.5.0
expect_ok "prerelease-0.4.1-rc.1" verify "$TMP/prerelease-0.4.1-rc.1" "$TMP/meta-0.4.1-rc.1.json" v0.4.0 --prerelease v0.4.1-rc.1

echo "== happy paths (pre-Linux <= 0.4.0) =="
expect_ok "pre-linux-0.4.0" verify "$TMP/release-0.4.0" "$TMP/meta-0.4.0.json" v0.5.0 v0.4.0
expect_ok "pre-linux-0.4.0-latest" verify "$TMP/release-0.4.0" "$TMP/meta-0.4.0.json" v0.4.0 v0.4.0
expect_ok "pre-linux-0.4.0-rc.1" verify "$TMP/prerelease-0.4.0-rc.1" "$TMP/meta-0.4.0-rc.1.json" v0.4.0 --prerelease v0.4.0-rc.1

echo "== failures =="
expect_fail "linux-era-missing-linux-assets" "ice-box_0.5.0_amd64.deb" \
  verify "$TMP/release-0.5.0-no-linux" "$TMP/meta-0.5.0-no-linux.json" v0.5.0 v0.5.0
expect_fail "linux-era-prerelease-missing-linux" "ice-box_0.4.1-rc.1_aarch64.AppImage.sig" \
  verify "$TMP/prerelease-0.4.1-rc.1-no-linux" "$TMP/meta-0.4.1-rc.1-no-linux.json" v0.4.0 --prerelease v0.4.1-rc.1
expect_fail "pre-linux-unexpected-linux-assets" "unexpected assets" \
  verify "$TMP/release-0.4.0-with-linux" "$TMP/meta-0.4.0-with-linux.json" v0.4.0 v0.4.0

cp -R "$RELEASE" "$TMP/tampered"
printf 'tampered\n' >>"$TMP/tampered/NOTICE"
expect_fail "checksum-mismatch" "does not match SHA256SUMS" \
  verify "$TMP/tampered" "$TMP/meta-final.json" v0.5.0 v0.5.0

cp -R "$RELEASE" "$TMP/missing-file"
rm "$TMP/missing-file/NOTICE"
expect_fail "missing-file" "could not download" \
  verify "$TMP/missing-file" "$TMP/meta-final.json" v0.5.0 v0.5.0

cp -R "$RELEASE" "$TMP/sums-missing"
python3 - "$TMP/sums-missing/SHA256SUMS" <<'PY'
import sys

path = sys.argv[1]
lines = [line for line in open(path, encoding="utf-8") if "  NOTICE" not in line]
open(path, "w", encoding="utf-8").writelines(lines)
PY
expect_fail "sums-missing-entry" "does not list: NOTICE" \
  verify "$TMP/sums-missing" "$TMP/meta-final.json" v0.5.0 v0.5.0

expect_fail "prerelease-with-latest" "must not publish latest.json" \
  verify "$RELEASE" "$TMP/meta-prerelease-with-latest.json" v0.5.1 --prerelease v0.5.0
expect_fail "prerelease-marked-latest" "marked as GitHub's latest release" \
  verify "$TMP/prerelease-0.5.0" "$TMP/meta-prerelease.json" v0.5.0 --prerelease v0.5.0

cp -R "$RELEASE" "$TMP/extra-asset"
printf 'extra\n' >"$TMP/extra-asset/extra.txt"
expect_fail "extra-asset" "unexpected assets" \
  verify "$TMP/extra-asset" "$TMP/meta-extra.json" v0.5.0 v0.5.0

cp -R "$RELEASE" "$TMP/extra-sums"
printf '%064d  unexpected.txt\n' 0 >>"$TMP/extra-sums/SHA256SUMS"
expect_fail "extra-sums-entry" "SHA256SUMS lists unexpected files" \
  verify "$TMP/extra-sums" "$TMP/meta-final.json" v0.5.0 v0.5.0

echo "test-verify-release-assets: OK"
