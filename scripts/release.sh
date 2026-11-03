#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Prepare a formal release PR in one command (docs/release-process.md, steps 1-3):
#
#   1. start release/vX.Y.Z from an up-to-date main
#   2. scripts/bump-version.sh (version sources + CHANGELOG.md Unreleased roll)
#   3. scripts/gate-local.sh --full and scripts/capture-docs.sh
#   4. commit the release files, push the branch, and open the PR
#
# After the PR is merged and CI on main is green, start the release from the
# Actions tab or with:
#   gh workflow run release.yml -f tag=vX.Y.Z -f mode=release
#
# Usage: scripts/release.sh <X.Y.Z[-suffix]> [--no-changelog] [--no-push]
#   --no-changelog  passed to bump-version.sh (the version section already exists)
#   --no-push       stop after the local commit; push and open the PR yourself
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

VERSION=""
BUMP_ARGS=()
PUSH=1
for arg in "$@"; do
  case "$arg" in
  --no-changelog) BUMP_ARGS+=(--no-changelog) ;;
  --no-push) PUSH=0 ;;
  -h | --help)
    sed -n '3,17p' "$0" | sed 's/^# \{0,1\}//'
    exit 0
    ;;
  -*)
    echo "release: unknown argument: $arg" >&2
    exit 2
    ;;
  *)
    if [[ -n "$VERSION" ]]; then
      echo "release: unexpected argument: $arg" >&2
      exit 2
    fi
    VERSION="${arg#v}"
    ;;
  esac
done
if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]; then
  echo "usage: $0 <X.Y.Z[-suffix]> [--no-changelog] [--no-push]" >&2
  exit 2
fi
TAG="v$VERSION"
BRANCH="release/$TAG"

die() {
  echo "release: $*" >&2
  exit 1
}

# Files a release commit may change. Anything else left dirty by the gate or
# the captures stops the script instead of being committed silently.
RELEASE_FILES=(
  CHANGELOG.md
  Cargo.toml
  Cargo.lock
  apps/desktop/package.json
  apps/mobile/package.json
  apps/mobile/src-tauri/tauri.conf.json
  docs/images/hero.png
  docs/images/hero.zh-CN.png
)

echo "== checking the working tree =="
[[ -z "$(git status --porcelain)" ]] || die "the working tree is not clean"
[[ "$(git branch --show-current)" == main ]] || die "run from main"
git fetch --quiet origin main --tags
[[ "$(git rev-parse HEAD)" == "$(git rev-parse origin/main)" ]] ||
  die "main is not at origin/main; pull first"
if git rev-parse --verify --quiet "refs/tags/$TAG" >/dev/null; then
  die "tag $TAG already exists; versions are never reused"
fi
if git ls-remote --exit-code --heads origin "$BRANCH" >/dev/null 2>&1; then
  die "branch $BRANCH already exists on origin"
fi

git switch -c "$BRANCH"

echo "== bump version =="
bash scripts/bump-version.sh "$VERSION" "${BUMP_ARGS[@]}"

echo "== release preflight (local) =="
# Without --latest-tag/--existing-tag: the newer-than checks need the release
# list and run in the workflow; this catches version and changelog mistakes.
bash scripts/release-preflight.sh "$TAG"

echo "== full local gate =="
bash scripts/gate-local.sh --full

echo "== documentation captures =="
bash scripts/capture-docs.sh

unexpected="$(
  git status --porcelain --untracked-files=all | cut -c4- |
    grep -Fvx -f <(printf '%s\n' "${RELEASE_FILES[@]}") || true
)"
if [[ -n "$unexpected" ]]; then
  die "unexpected changes outside the release files (commit or discard them first):
$unexpected"
fi

git add -- "${RELEASE_FILES[@]}"
git commit -m "release: $TAG"

if [[ "$PUSH" == 0 ]]; then
  echo "release: committed on $BRANCH; push it and open a PR to main when ready"
  exit 0
fi

git push -u origin "$BRANCH"
gh pr create --base main --head "$BRANCH" --title "release: $TAG" --body "$(
  cat <<EOF
Release $TAG.

- Version sources bumped with \`scripts/bump-version.sh\`
- CHANGELOG.md section: \`$(bash scripts/release-notes.sh "$TAG" | tail -n 1 | sed 's/.*(\(.*\))/\1/')\`
- \`scripts/gate-local.sh --full\` and \`scripts/capture-docs.sh\` ran locally

After merge and a green CI run on main:
\`gh workflow run release.yml -f tag=$TAG -f mode=release\`
EOF
)"

echo "release: PR opened for $TAG"
