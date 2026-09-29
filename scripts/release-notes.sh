#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Print the short GitHub Release body for a tag like v0.1.1 (or plain 0.1.1).
#
# Formal release bodies stay terse: one line about the release plus a link to
# the version's section in CHANGELOG.md. The full notes live in the changelog
# and are never pasted into the release body (see docs/release-process.md).
# Usage: scripts/release-notes.sh v0.1.1
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CHANGELOG="$ROOT/CHANGELOG.md"
REPO_URL="https://github.com/yilong-musk/ice-box"

VERSION="${1:-}"
VERSION="${VERSION#v}"
if [[ -z "$VERSION" ]]; then
  echo "usage: $0 <version, e.g. v0.1.1>" >&2
  exit 1
fi

if [[ ! -f "$CHANGELOG" ]]; then
  echo "missing $CHANGELOG" >&2
  exit 1
fi

VERSION_RE="${VERSION//./\\.}"
HEADING="$(grep -m1 -E "^## \[$VERSION_RE\] - [0-9]{4}-[0-9]{2}-[0-9]{2}$" "$CHANGELOG" || true)"
if [[ -z "$HEADING" ]]; then
  echo "no CHANGELOG section for $VERSION (expected '## [$VERSION] - YYYY-MM-DD')" >&2
  exit 1
fi
DATE="${HEADING##* - }"

SECTION_BODY="$(awk -v section="## [$VERSION]" '
  index($0, section) == 1 { in_section = 1; next }
  in_section && /^## / { exit }
  in_section { print }
' "$CHANGELOG")"

if [[ -z "${SECTION_BODY//[[:space:]]/}" ]]; then
  echo "empty CHANGELOG section for $VERSION" >&2
  exit 1
fi

# GitHub's anchor for "## [x.y.z] - YYYY-MM-DD" drops the brackets and dots and
# turns spaces into hyphens, e.g. `#0115---2026-09-28`.
ANCHOR="$(printf '%s - %s' "$VERSION" "$DATE" \
  | tr '[:upper:]' '[:lower:]' \
  | sed -e 's/[^a-z0-9 -]//g' -e 's/ /-/g')"

cat <<EOF
**ice-box v$VERSION** — $DATE

macOS (Apple Silicon) and Windows installers are attached below.

Full changelog: [CHANGELOG.md › $VERSION]($REPO_URL/blob/main/CHANGELOG.md#$ANCHOR)
EOF
