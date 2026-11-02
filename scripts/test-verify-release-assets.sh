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

RELEASE="$TMP/release"
mkdir -p "$RELEASE"
printf 'dmg\n' >"$RELEASE/ice-box_0.2.6_aarch64.dmg"
printf 'tar\n' >"$RELEASE/ice-box.app.tar.gz"
printf 'sig\n' >"$RELEASE/ice-box.app.tar.gz.sig"
printf 'exe\n' >"$RELEASE/ice-box_0.2.6_x64-setup.exe"
printf 'sig\n' >"$RELEASE/ice-box_0.2.6_x64-setup.exe.sig"
printf 'apk\n' >"$RELEASE/ice-box_0.2.6_android_arm64.apk"
printf 'license\n' >"$RELEASE/LICENSE"
printf 'notice\n' >"$RELEASE/NOTICE"
printf 'sing-box license\n' >"$RELEASE/sing-box-LICENSE"
cat >"$RELEASE/latest.json" <<'EOF'
{
  "version": "0.2.6",
  "platforms": {
    "darwin-aarch64": {
      "signature": "sig",
      "url": "https://github.com/fixture/repo/releases/download/v0.2.6/ice-box.app.tar.gz"
    },
    "windows-x86_64": {
      "signature": "sig",
      "url": "https://github.com/fixture/repo/releases/download/v0.2.6/ice-box_0.2.6_x64-setup.exe"
    }
  }
}
EOF

python3 - "$RELEASE" "$TMP/meta-final.json" "$TMP/meta-prerelease.json" \
  "$TMP/meta-prerelease-with-latest.json" <<'PY'
import hashlib
import json
import sys
from pathlib import Path

root = Path(sys.argv[1])
entries = []
for path in sorted(root.iterdir()):
    if path.name == "SHA256SUMS":
        continue
    entries.append((path.name, hashlib.sha256(path.read_bytes()).hexdigest()))
(root / "SHA256SUMS").write_text(
    "".join(f"{digest}  {name}\n" for name, digest in entries), encoding="utf-8"
)

names = [name for name, _ in entries] + ["SHA256SUMS"]


def meta(prerelease: bool, assets: list[str]) -> dict:
    return {
        "isDraft": False,
        "isPrerelease": prerelease,
        "assets": [{"name": name} for name in assets],
    }


Path(sys.argv[2]).write_text(json.dumps(meta(False, names)), encoding="utf-8")
Path(sys.argv[3]).write_text(
    json.dumps(meta(True, [name for name in names if name != "latest.json"])),
    encoding="utf-8",
)
Path(sys.argv[4]).write_text(json.dumps(meta(True, names)), encoding="utf-8")
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

echo "== happy paths =="
expect_ok "latest" verify "$RELEASE" "$TMP/meta-final.json" v0.2.6 v0.2.6
expect_ok "older-tag" verify "$RELEASE" "$TMP/meta-final.json" v0.2.7 v0.2.6
expect_ok "no-latest-release" verify "$RELEASE" "$TMP/meta-final.json" "" v0.2.6
expect_ok "prerelease" verify "$RELEASE" "$TMP/meta-prerelease.json" v0.2.7 --prerelease v0.2.6

echo "== failures =="
cp -R "$RELEASE" "$TMP/tampered"
printf 'tampered\n' >>"$TMP/tampered/NOTICE"
expect_fail "checksum-mismatch" "does not match SHA256SUMS" \
  verify "$TMP/tampered" "$TMP/meta-final.json" v0.2.6 v0.2.6

cp -R "$RELEASE" "$TMP/missing-file"
rm "$TMP/missing-file/NOTICE"
expect_fail "missing-file" "could not download" \
  verify "$TMP/missing-file" "$TMP/meta-final.json" v0.2.6 v0.2.6

cp -R "$RELEASE" "$TMP/sums-missing"
python3 - "$TMP/sums-missing/SHA256SUMS" <<'PY'
import sys

path = sys.argv[1]
lines = [line for line in open(path, encoding="utf-8") if "  NOTICE" not in line]
open(path, "w", encoding="utf-8").writelines(lines)
PY
expect_fail "sums-missing-entry" "does not list: NOTICE" \
  verify "$TMP/sums-missing" "$TMP/meta-final.json" v0.2.6 v0.2.6

expect_fail "prerelease-with-latest" "must not publish latest.json" \
  verify "$RELEASE" "$TMP/meta-prerelease-with-latest.json" v0.2.7 --prerelease v0.2.6
expect_fail "prerelease-marked-latest" "marked as GitHub's latest release" \
  verify "$RELEASE" "$TMP/meta-prerelease.json" v0.2.6 --prerelease v0.2.6

echo "test-verify-release-assets: OK"
