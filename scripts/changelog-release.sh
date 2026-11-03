#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Turn the CHANGELOG.md "## [Unreleased]" section into the release section for
# a version and leave a fresh, empty "## [Unreleased]" heading above it.
#
# Usage: scripts/changelog-release.sh <version> [--date YYYY-MM-DD] [--file PATH]
#
# Fails when the Unreleased section is missing or empty, or when the version
# already has a section. Called by scripts/bump-version.sh.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

usage() {
  echo "usage: $0 <version> [--date YYYY-MM-DD] [--file PATH]" >&2
}

VERSION=""
DATE="$(date -u +%Y-%m-%d)"
FILE="$ROOT/CHANGELOG.md"
while (($# > 0)); do
  case "$1" in
  --date)
    DATE="${2:-}"
    shift 2 || { usage; exit 2; }
    ;;
  --file)
    FILE="${2:-}"
    shift 2 || { usage; exit 2; }
    ;;
  -h | --help)
    usage
    exit 0
    ;;
  -*)
    echo "unknown argument: $1" >&2
    usage
    exit 2
    ;;
  *)
    if [[ -n "$VERSION" ]]; then
      echo "unexpected argument: $1" >&2
      usage
      exit 2
    fi
    VERSION="${1#v}"
    shift
    ;;
  esac
done

if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]; then
  usage
  exit 2
fi
if [[ ! "$DATE" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]]; then
  echo "invalid date '$DATE' (expected YYYY-MM-DD)" >&2
  exit 2
fi
if [[ ! -f "$FILE" ]]; then
  echo "missing $FILE" >&2
  exit 1
fi

python3 - "$FILE" "$VERSION" "$DATE" <<'PY'
import re
import sys
from pathlib import Path

path, version, date = Path(sys.argv[1]), sys.argv[2], sys.argv[3]
lines = path.read_text(encoding="utf-8").splitlines(keepends=True)

if any(line.startswith(f"## [{version}]") for line in lines):
    sys.exit(f"CHANGELOG.md already has a section for {version}")

starts = [i for i, line in enumerate(lines) if line.rstrip("\n") == "## [Unreleased]"]
if len(starts) != 1:
    sys.exit("CHANGELOG.md must have exactly one '## [Unreleased]' heading")
start = starts[0]
end = next(
    (i for i in range(start + 1, len(lines)) if lines[i].startswith("## ")),
    len(lines),
)
body = lines[start + 1 : end]
if not "".join(body).strip():
    sys.exit(
        "the Unreleased section of CHANGELOG.md is empty; add the release notes "
        "there first (or pass --no-changelog to bump-version.sh)"
    )
if not re.search(r"^### ", "".join(body), re.MULTILINE):
    sys.exit("the Unreleased section has no '### Added/Changed/Fixed/...' subsection")

new = lines[:start] + ["## [Unreleased]\n", "\n", f"## [{version}] - {date}\n"] + body + lines[end:]
path.write_text("".join(new), encoding="utf-8")
PY

echo "changelog-release: moved Unreleased to [$VERSION] - $DATE"
