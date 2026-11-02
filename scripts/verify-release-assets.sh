#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Verify a published GitHub release, its checksums, and its updater feed.
# Usage: scripts/verify-release-assets.sh [--prerelease] <tag>
#
# Requires gh (GH_TOKEN). Run after the release is published; every asset is
# downloaded and checked against SHA256SUMS. The final-release path retries the
# latest.json endpoint to absorb CDN lag. When the tag is not GitHub's latest
# release (a rerun of an older release), it checks the latest.json asset
# attached to that release instead.
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
import concurrent.futures
import hashlib
import http.client
import json
import os
import re
import subprocess
import sys
import time
import urllib.request

repo, tag, prerelease = sys.argv[1], sys.argv[2], sys.argv[3] == "1"
version = tag[1:]
retry_attempts = int(os.environ.get("ICE_BOX_RELEASE_RETRY_ATTEMPTS", "6"))
retry_delay = float(os.environ.get("ICE_BOX_RELEASE_RETRY_DELAY", "5"))
# The overrides let scripts/test-verify-release-assets.sh serve a fixture with
# file:// URLs instead of reaching GitHub.
base = os.environ.get(
    "ICE_BOX_RELEASE_BASE_URL", f"https://github.com/{repo}/releases/download/{tag}"
)
latest_url = os.environ.get(
    "ICE_BOX_RELEASE_LATEST_URL",
    f"https://github.com/{repo}/releases/latest/download/latest.json",
)


def fail(message: str) -> None:
    print(message, file=sys.stderr)
    sys.exit(1)


def gh(*args: str) -> str:
    return subprocess.check_output(["gh", *args], text=True)


def fetch(url: str, *, timeout: int = 60) -> bytes:
    request = urllib.request.Request(url, headers={"User-Agent": "ice-box-release-check"})
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return response.read()


def sha256(url: str) -> str:
    digest = hashlib.sha256()
    request = urllib.request.Request(url, headers={"User-Agent": "ice-box-release-check"})
    with urllib.request.urlopen(request, timeout=600) as response:
        while True:
            chunk = response.read(1024 * 1024)
            if not chunk:
                break
            digest.update(chunk)
    return digest.hexdigest()


def fetch_retry(url: str) -> bytes:
    last = None
    for _ in range(retry_attempts):
        try:
            return fetch(url)
        except (OSError, http.client.HTTPException) as error:
            last = error
            time.sleep(retry_delay)
    raise last


def sha256_retry(url: str, expected: str) -> str:
    # A freshly published asset can briefly return content that does not match
    # the checksum (CDN lag). Retry both download errors and digest mismatches
    # so the post-publish check does not fail on a transient response.
    last = None
    digest = ""
    for _ in range(retry_attempts):
        try:
            digest = sha256(url)
        except (OSError, http.client.HTTPException) as error:
            last = error
        else:
            if digest == expected:
                return digest
        time.sleep(retry_delay)
    if not digest:
        raise last
    return digest


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
extra = sorted(names - expected)
if extra:
    fail(f"{tag} has unexpected assets: {', '.join(extra)}")

try:
    sums = fetch_retry(f"{base}/SHA256SUMS").decode("utf-8")
except (OSError, http.client.HTTPException, UnicodeDecodeError):
    fail("could not download SHA256SUMS")

sums_map: dict[str, str] = {}
for line in sums.splitlines():
    match = re.fullmatch(r"([0-9a-f]{64})  (.+)", line)
    if match:
        sums_map[match.group(2)] = match.group(1)

unlisted = sorted(expected - {"SHA256SUMS"} - set(sums_map))
if unlisted:
    fail(f"SHA256SUMS does not list: {', '.join(unlisted)}")
extra_sums = sorted(set(sums_map) - (expected - {"SHA256SUMS"}))
if extra_sums:
    fail(f"SHA256SUMS lists unexpected files: {', '.join(extra_sums)}")

assets = sorted(expected - {"SHA256SUMS"})
results: dict[str, str] = {}
errors: list[str] = []
with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
    futures = {
        pool.submit(sha256_retry, f"{base}/{name}", sums_map[name]): name
        for name in assets
    }
    for future in concurrent.futures.as_completed(futures):
        name = futures[future]
        try:
            results[name] = future.result()
        except (OSError, http.client.HTTPException) as error:
            errors.append(f"{name}: could not download ({error})")
if errors:
    fail("; ".join(errors))
for name in assets:
    if results[name] != sums_map[name]:
        fail(f"{name}: sha256 {results[name]} does not match SHA256SUMS {sums_map[name]}")
    print(f"asset OK: {name}")

latest = ""
for _ in range(retry_attempts):
    try:
        latest = gh("api", f"repos/{repo}/releases/latest", "--jq", ".tag_name").strip()
        break
    except subprocess.CalledProcessError:
        time.sleep(retry_delay)

if prerelease:
    if latest == tag:
        fail(f"pre-release {tag} is marked as GitHub's latest release")
    print(f"pre-release is not the latest stable release (latest: {latest or 'none'})")
else:
    if latest == tag:
        endpoint = latest_url
        source = "the updater endpoint"
    else:
        # Rerunning an older release: /releases/latest points at a newer tag,
        # so verify the latest.json asset attached to this release instead.
        endpoint = f"{base}/latest.json"
        source = f"the {tag} latest.json asset"
    payload = None
    for _ in range(12):
        try:
            payload = json.loads(fetch(endpoint))
        except (OSError, json.JSONDecodeError, http.client.HTTPException, UnicodeDecodeError):
            payload = None
        if payload and payload.get("version") == version:
            break
        time.sleep(5)
    if not payload or payload.get("version") != version:
        fail(f"{source} does not report version {version}")
    platforms = payload.get("platforms") or {}
    for name in ("darwin-aarch64", "windows-x86_64"):
        platform = platforms.get(name) or {}
        if not (platform.get("signature") or "").strip():
            fail(f"latest.json {name} signature is empty")
        if f"/{tag}/" not in (platform.get("url") or ""):
            fail(f"latest.json {name} url does not point at {tag}")
    print(f"latest.json reports {version} for darwin-aarch64 and windows-x86_64 ({source})")

print(f"verify-release-assets: OK ({tag})")
PY
