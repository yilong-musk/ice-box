#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Shared release-version validation. Source this file; do not run it.
#
# Every release script (release.sh, bump-version.sh, changelog-release.sh,
# release-preflight.sh) and the release workflow accept the same format, so a
# version one step accepts is never rejected halfway through a later one:
#
#   X.Y.Z or X.Y.Z-suffix, semver without leading zeros in X, Y, Z or in a
#   numeric pre-release identifier (rc.1, not rc.01). An optional leading "v"
#   is accepted and stripped.

ICE_BOX_VERSION_RE='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$'

# release_version <raw>: print <raw> without its leading "v" when it is a valid
# release version; otherwise print the reason to stderr and return 1.
release_version() {
  local raw="${1:-}"
  local version="${raw#v}"
  if [[ ! "$version" =~ $ICE_BOX_VERSION_RE ]]; then
    echo "invalid version '$raw' (expected X.Y.Z or X.Y.Z-suffix without leading zeros)" >&2
    return 1
  fi
  local suffix="${BASH_REMATCH[4]:-}"
  if [[ -n "$suffix" ]]; then
    local identifier
    local -a identifiers
    IFS='.' read -r -a identifiers <<<"${suffix#-}"
    for identifier in "${identifiers[@]}"; do
      if [[ "$identifier" =~ ^0[0-9]+$ ]]; then
        echo "invalid version '$raw' (numeric prerelease identifier '$identifier' cannot have leading zeros)" >&2
        return 1
      fi
    done
  fi
  printf '%s\n' "$version"
}
