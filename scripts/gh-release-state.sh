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
# Requires gh and GH_TOKEN. Uses GraphQL because the REST "get a release by
# tag name" endpoint returns 404 for draft releases, which would make a rerun
# mistake an existing draft for a missing release. GraphQL returns the draft
# (and null for a missing tag) without matching human-readable error text.
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

tag = sys.argv[1]
kind = sys.argv[2] == "1"
repo = os.environ.get("GITHUB_REPOSITORY", "yilong-musk/ice-box")
if "/" not in repo:
    print(f"GITHUB_REPOSITORY is not owner/name: {repo!r}", file=sys.stderr)
    sys.exit(1)
owner, name = repo.split("/", 1)

query = (
    "query($owner:String!,$name:String!,$tag:String!){"
    "repository(owner:$owner,name:$name){release(tagName:$tag){"
    "databaseId isDraft isPrerelease}}}"
)
try:
    proc = subprocess.run(
        [
            "gh",
            "api",
            "graphql",
            "-f",
            f"query={query}",
            "-f",
            f"owner={owner}",
            "-f",
            f"name={name}",
            "-f",
            f"tag={tag}",
        ],
        text=True,
        capture_output=True,
    )
except OSError as error:
    print(f"cannot run gh: {error}", file=sys.stderr)
    sys.exit(1)

if proc.returncode != 0:
    sys.stderr.write(proc.stderr)
    sys.stderr.write(proc.stdout)
    sys.exit(1)

try:
    payload = json.loads(proc.stdout)
except json.JSONDecodeError as error:
    print(f"cannot parse release response: {error}", file=sys.stderr)
    sys.exit(1)

repository = (payload.get("data") or {}).get("repository")
if repository is None:
    print("release lookup did not return a repository", file=sys.stderr)
    sys.exit(1)
release = repository.get("release")
if release is None:
    print("missing")
    sys.exit(0)

if kind:
    if release.get("isDraft"):
        print("draft")
    elif release.get("isPrerelease"):
        print("prerelease")
    else:
        print("final")
else:
    print("true" if release.get("isDraft") else "false")
PY
