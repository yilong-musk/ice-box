#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Check that the sing-box version written in the documentation matches the
# pinned core in third_party/sing-box/VERSION, so a core upgrade cannot leave
# the README badge or the TUN guide on the old version.
#
# Usage: scripts/check-doc-versions.sh [repo-root]
#
# Checked places:
#   - README.md and README.zh-CN.md: the `sing--box-<version>-` badge
#   - docs/tun.md: the `sing-box **<version>**` line in the introduction
set -euo pipefail

ROOT="${1:-$(cd "$(dirname "$0")/.." && pwd)}"

version="$(tr -d '[:space:]' <"$ROOT/third_party/sing-box/VERSION")"
if [[ -z "$version" ]]; then
  echo "check-doc-versions: third_party/sing-box/VERSION is empty" >&2
  exit 1
fi

failed=0

# check <file> <extended regex with one version capture> <what>
check() {
  local file="$1" pattern="$2" what="$3" found
  found="$(sed -n -E "s/.*${pattern}.*/\1/p" "$ROOT/$file" | head -n 1)"
  if [[ -z "$found" ]]; then
    echo "check-doc-versions: $file: no $what found" >&2
    failed=1
  elif [[ "$found" != "$version" ]]; then
    echo "check-doc-versions: $file: $what says $found, pinned core is $version" >&2
    failed=1
  fi
}

check README.md 'sing--box-([0-9][0-9A-Za-z.]*)-' "sing-box badge"
check README.zh-CN.md 'sing--box-([0-9][0-9A-Za-z.]*)-' "sing-box badge"
check docs/tun.md 'sing-box \*\*([0-9][0-9A-Za-z.]*)\*\*' "sing-box version"

if [[ "$failed" != 0 ]]; then
  echo "check-doc-versions: update the documentation together with third_party/sing-box/VERSION" >&2
  exit 1
fi
echo "check-doc-versions: documentation matches sing-box $version"
