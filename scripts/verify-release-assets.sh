#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Verify a published GitHub release, its checksums, and its updater feed.
# Usage: scripts/verify-release-assets.sh [--prerelease] <tag>
#
# Requires gh (GH_TOKEN) and curl. Run after the release is published; the
# final-release path retries the latest.json endpoint to absorb CDN lag.
set -euo pipefail

PRERELEASE=0
TAG=""

usage() {
  cat >&2 <<'EOF'
usage: scripts/verify-release-assets.sh [--prerelease] <tag>
EOF
}

while (($# > 0)); do
  case "$1" in
  --prerelease)
    PRERELEASE=1
    shift
    ;;
  -h | --help)
    usage
    exit 0
    ;;
  *)
    if [[ -n "$TAG" ]]; then
      echo "unexpected argument: $1" >&2
      usage
      exit 2
    fi
    TAG="$1"
    shift
    ;;
  esac
done

if [[ -z "$TAG" ]]; then
  usage
  exit 2
fi
if [[ ! "$TAG" =~ ^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$ ]]; then
  echo "invalid release tag '$TAG'" >&2
  exit 1
fi

REPO="${GITHUB_REPOSITORY:-yilong-musk/ice-box}"
python3 - "$REPO" "$TAG" "$PRERELEASE" <<'PY'
import json
import re
import subprocess
import sys
import time
import urllib.request

repo, tag, prerelease = sys.argv[1], sys.argv[2], sys.argv[3] == "1"
version = tag[1:]
base = f"https://github.com/{repo}/releases/download/{tag}"


def fail(message: str) -> None:
    print(message, file=sys.stderr)
    sys.exit(1)


def gh(*args: str) -> str:
    return subprocess.check_output(["gh", *args], text=True)


def fetch(url: str, *, timeout: int = 60) -> bytes:
    request = urllib.request.Request(url, headers={"User-Agent": "ice-box-release-check"})
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return response.read()


def head(url: str) -> None:
    subprocess.run(
        [
            "curl",
            "-fsSIL",
            "--retry",
            "5",
            "--retry-all-errors",
            "--retry-delay",
            "2",
            "--max-time",
            "60",
            url,
        ],
        check=True,
        stdout=subprocess.DEVNULL,
    )


expected = {
    f"ice-box_{version}_aarch64.dmg",
    "ice-box.app.tar.gz",
    "ice-box.app.tar.gz.sig",
    f"ice-box_{version}_x64-setup.exe",
    f"ice-box_{version}_x64-setup.exe.sig",
    f"ice-box_{version}_android_arm64.apk",
    "SHA256SUMS",
    "LICENSE",
    "NOTICE",
    "sing-box-LICENSE",
}
if not prerelease:
    expected.add("latest.json")

meta = json.loads(gh("release", "view", tag, "--json", "isDraft,isPrerelease,assets"))
if meta["isDraft"]:
    fail(f"{tag} is still a draft")
if meta["isPrerelease"] != prerelease:
    kind = "a pre-release" if prerelease else "a final release"
    fail(f"{tag} must be published as {kind}")

names = {asset["name"] for asset in meta["assets"]}
missing = sorted(expected - names)
if missing:
    fail(f"{tag} is missing assets: {', '.join(missing)}")
if prerelease and "latest.json" in names:
    fail(f"pre-release {tag} must not publish latest.json")

for name in sorted(expected):
    head(f"{base}/{name}")
    print(f"asset URL OK: {name}")

sums = ""
for _ in range(12):
    try:
        sums = fetch(f"{base}/SHA256SUMS").decode("utf-8")
        break
    except OSError:
        time.sleep(5)
if not sums:
    fail("could not download SHA256SUMS")
for name in sorted(expected - {"SHA256SUMS"}):
    if not re.search(rf"^[0-9a-f]{{64}}  {re.escape(name)}$", sums, re.MULTILINE):
        fail(f"SHA256SUMS does not list {name}")

if prerelease:
    try:
        latest = gh("api", f"repos/{repo}/releases/latest", "--jq", ".tag_name").strip()
    except subprocess.CalledProcessError:
        latest = ""
    if latest == tag:
        fail(f"pre-release {tag} is marked as GitHub's latest release")
    print(f"pre-release is not the latest stable release (latest: {latest or 'none'})")
else:
    payload = None
    for _ in range(12):
        try:
            payload = json.loads(fetch(f"https://github.com/{repo}/releases/latest/download/latest.json"))
        except (OSError, json.JSONDecodeError):
            payload = None
        if payload and payload.get("version") == version:
            break
        time.sleep(5)
    if not payload or payload.get("version") != version:
        fail(f"latest.json does not report version {version}")
    platforms = payload.get("platforms") or {}
    for name in ("darwin-aarch64", "windows-x86_64"):
        platform = platforms.get(name) or {}
        if not (platform.get("signature") or "").strip():
            fail(f"latest.json {name} signature is empty")
        if f"/{tag}/" not in (platform.get("url") or ""):
            fail(f"latest.json {name} url does not point at {tag}")
    print(f"latest.json reports {version} for darwin-aarch64 and windows-x86_64")

print(f"verify-release-assets: OK ({tag})")
PY
