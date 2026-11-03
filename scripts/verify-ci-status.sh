#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Check that the required CI checks passed on a commit before it is released.
# The release workflow builds from this commit without re-running the gate, so
# the gate result it relies on is the CI run on main for the same SHA.
#
# Usage: scripts/verify-ci-status.sh <commit-sha>
# Needs GH_TOKEN with `checks: read` and `contents: read`, and
# GITHUB_REPOSITORY (owner/repo).
#
# The required check names are read from GitHub, not listed here: the status
# checks required on `main` by its branch protection and by any branch ruleset.
# A rename in ci.yml therefore only has to be mirrored in those settings, and
# a check added there is verified here too. No required check at all is an
# error, so a missing setting cannot let an unchecked commit through.
set -euo pipefail

SHA="${1:-}"
if [[ ! "$SHA" =~ ^[0-9a-f]{40}$ ]]; then
  echo "usage: $0 <40-character commit sha>" >&2
  exit 2
fi
REPO="${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required}"

BRANCH=main

# Classic branch protection exposes its required contexts on the branch
# itself, which only needs read access (the /protection endpoint needs admin).
# Rulesets that apply to the branch add their own required checks.
required="$(
  {
    gh api "repos/$REPO/branches/$BRANCH" \
      --jq '.protection.required_status_checks.contexts // [] | .[]'
    gh api --paginate "repos/$REPO/rules/branches/$BRANCH" \
      --jq '.[] | select(.type == "required_status_checks") |
        .parameters.required_status_checks[].context'
  } | sed '/^$/d' | sort -u
)"
if [[ -z "$required" ]]; then
  echo "verify-ci-status: no status checks are required on $BRANCH; configure branch protection (docs/release-process.md)" >&2
  exit 1
fi
REQUIRED=()
while IFS= read -r name; do
  REQUIRED+=("$name")
done <<<"$required"

# One line per check run: id, name, status, conclusion.
runs="$(gh api --paginate "repos/$REPO/commits/$SHA/check-runs?per_page=100" \
  --jq '.check_runs[] | [.id, .name, .status, (.conclusion // "")] | @tsv')"

failed=0
for name in "${REQUIRED[@]}"; do
  # A re-run creates a new check run with a higher id; the newest one counts.
  latest="$(printf '%s\n' "$runs" | awk -F'\t' -v n="$name" '$2 == n' | sort -t$'\t' -k1,1n | tail -n 1)"
  if [[ -z "$latest" ]]; then
    echo "verify-ci-status: '$name' has not run on $SHA" >&2
    failed=1
    continue
  fi
  IFS=$'\t' read -r _ _ status conclusion <<<"$latest"
  if [[ "$status" != completed ]]; then
    echo "verify-ci-status: '$name' is still $status on $SHA; wait for CI to finish" >&2
    failed=1
  elif [[ "$conclusion" != success ]]; then
    echo "verify-ci-status: '$name' concluded '$conclusion' on $SHA" >&2
    failed=1
  else
    echo "verify-ci-status: '$name' passed"
  fi
done

if [[ "$failed" != 0 ]]; then
  echo "verify-ci-status: CI on main has not passed for $SHA; re-run or fix CI before releasing it" >&2
  exit 1
fi
echo "verify-ci-status: OK ($SHA)"
