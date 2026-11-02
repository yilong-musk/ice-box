#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Print whether a GitHub release exists and is a draft.
# Usage: scripts/gh-release-state.sh [--kind] <tag>
#
# Outputs one of:
#   true     release exists and is a draft
#   false    release exists and is published (final or pre-release)
#   missing  no release for the tag
#
# With --kind, outputs draft, prerelease, final, or missing instead. The
# website deploy uses --kind to distinguish a published final release from a
# pre-release without a second API call.
#
# Requires gh and GH_TOKEN. This distinguishes a 404 from other API errors
# instead of matching the human-readable text of `gh release view` errors.
set -euo pipefail

KIND=0
if [[ "${1:-}" == "--kind" ]]; then
  KIND=1
  shift
fi

TAG="${1:-}"
if [[ -z "$TAG" || "$TAG" == "-h" || "$TAG" == "--help" ]]; then
  echo "usage: $0 [--kind] <tag>" >&2
  exit 2
fi

python3 - "$TAG" "$KIND" <<'PY'
import json
import os
import subprocess
import sys
import urllib.parse

tag = sys.argv[1]
kind = sys.argv[2] == "1"
repo = os.environ.get("GITHUB_REPOSITORY", "yilong-musk/ice-box")
url = f"repos/{repo}/releases/tags/{urllib.parse.quote(tag, safe='')}"
try:
    proc = subprocess.run(
        ["gh", "api", "-i", url],
        text=True,
        capture_output=True,
    )
except OSError as error:
    print(f"cannot run gh: {error}", file=sys.stderr)
    sys.exit(1)

response = proc.stdout.replace("\r\n", "\n")
header, _, body = response.partition("\n\n")
parts = header.split()
status = parts[1] if len(parts) > 1 else ""

if proc.returncode == 0:
    try:
        release = json.loads(body)
    except json.JSONDecodeError as error:
        print(f"cannot parse release response: {error}", file=sys.stderr)
        sys.exit(1)
    if kind:
        if release.get("isDraft"):
            print("draft")
        elif release.get("isPrerelease"):
            print("prerelease")
        else:
            print("final")
    else:
        print("true" if release.get("isDraft") else "false")
    sys.exit(0)

if status == "404":
    print("missing")
    sys.exit(0)

sys.stderr.write(proc.stderr)
sys.stderr.write(proc.stdout)
sys.exit(1)
PY
