#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Print the Android versionCode for a semantic version.
# Usage: scripts/android-version-code.sh 0.1.15
#
# This is the integer Tauri writes when bundle.android.versionCode is unset:
# major * 1000000 + minor * 1000 + patch. A pre-release suffix is ignored.
set -euo pipefail

VERSION="${1:-}"
if [[ ! "$VERSION" =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)(-[0-9A-Za-z.-]+)?$ ]]; then
  echo "usage: $0 <semver, e.g. 0.1.15>" >&2
  exit 1
fi

MAJOR="${BASH_REMATCH[1]}"
MINOR="${BASH_REMATCH[2]}"
PATCH="${BASH_REMATCH[3]}"
CODE=$((MAJOR * 1000000 + MINOR * 1000 + PATCH))
if ((CODE < 1 || CODE > 2100000000)); then
  echo "version code $CODE is outside 1..2100000000" >&2
  exit 1
fi
printf '%s\n' "$CODE"
