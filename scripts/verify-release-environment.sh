#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Check that the `release` environment's deployment branch/tag rules allow the
# ref this run deploys from, before the platform builds spend an hour.
#
# Tag pushes deploy from refs/tags/vX.Y.Z, but the manual `release` and
# `rehearse` modes deploy from the branch the run was started on (main). Rules
# that only allow `v*` tags would reject the publish job after every build
# finished; this turns that into a preflight failure with the fix in it.
#
# Usage: scripts/verify-release-environment.sh <ref> [environment]
#   <ref>  the run's GITHUB_REF, e.g. refs/heads/main or refs/tags/v0.2.8
# Needs GH_TOKEN with `actions: read` and GITHUB_REPOSITORY (owner/repo).
set -euo pipefail

REF="${1:-}"
ENVIRONMENT="${2:-release}"
REPO="${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required}"
case "$REF" in
refs/heads/?*) kind=branch name="${REF#refs/heads/}" ;;
refs/tags/?*) kind=tag name="${REF#refs/tags/}" ;;
*)
  echo "usage: $0 <refs/heads/NAME|refs/tags/NAME> [environment]" >&2
  exit 2
  ;;
esac

reject() {
  echo "verify-release-environment: the '$ENVIRONMENT' environment does not allow $kind '$name' ($*)." >&2
  echo "  The publish job would be rejected after the builds. Allow both the v* tag" >&2
  echo "  pattern and main in Settings -> Environments -> $ENVIRONMENT (docs/release-process.md)." >&2
  exit 1
}

err="$(mktemp)"
trap 'rm -f "$err"' EXIT
if ! policy="$(gh api "repos/$REPO/environments/$ENVIRONMENT" \
  --jq '.deployment_branch_policy // {} |
    "\(.protected_branches // false) \(.custom_branch_policies // false)"' 2>"$err")"; then
  if grep -q 'HTTP 404' "$err"; then
    # GitHub creates a missing environment on first use, without any rules.
    echo "verify-release-environment: '$ENVIRONMENT' does not exist yet; no deployment rules"
    exit 0
  fi
  cat "$err" >&2
  exit 1
fi

read -r protected_only custom <<<"$policy"
if [[ "$protected_only" == true ]]; then
  # "Protected branches only" admits no tags, and only protected branches.
  [[ "$kind" == branch ]] || reject "only protected branches may deploy"
  protected="$(gh api "repos/$REPO/branches/$name" --jq '.protected')"
  [[ "$protected" == true ]] || reject "only protected branches may deploy"
elif [[ "$custom" == true ]]; then
  allowed=0
  while IFS=$'\t' read -r type pattern; do
    [[ -n "$pattern" && "$type" == "$kind" ]] || continue
    # GitHub matches with File.fnmatch; a shell glob is the same for the
    # patterns used here (main, release/*, v*).
    # shellcheck disable=SC2053
    if [[ "$name" == $pattern ]]; then
      allowed=1
      break
    fi
  done < <(gh api --paginate "repos/$REPO/environments/$ENVIRONMENT/deployment-branch-policies" \
    --jq '.branch_policies[] | [(.type // "branch"), .name] | @tsv')
  [[ "$allowed" == 1 ]] || reject "no matching $kind rule"
fi

echo "verify-release-environment: '$ENVIRONMENT' allows $kind '$name'"
