#!/usr/bin/env bash
# Print the CHANGELOG.md section for a tag like v0.1.1 (or plain 0.1.1).
# Usage: scripts/release-notes.sh v0.1.1
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CHANGELOG="$ROOT/CHANGELOG.md"

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

SECTION="## [$VERSION]"
OUT="$(awk -v section="$SECTION" '
  index($0, section) == 1 { in_section = 1; next }
  in_section && /^## / { exit }
  in_section { print }
' "$CHANGELOG")"

if [[ -z "$OUT" ]]; then
  echo "no CHANGELOG section for $VERSION (expected '$SECTION - YYYY-MM-DD')" >&2
  exit 1
fi

printf '%s\n' "$OUT"