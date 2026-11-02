#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Validate a release tag before the release pipeline spends build time.
# Usage: scripts/release-preflight.sh <tag> [--latest-tag <tag>] [--allow-existing]
#
# Checks:
#   - the tag is vX.Y.Z with an optional pre-release suffix
#   - minor and patch fit the Android versionCode scheme (below 1000)
#   - every version source matches the tag
#   - CHANGELOG.md has a non-empty section for the version
#   - the version is newer than --latest-tag unless --allow-existing is set
#
# The workflow additionally checks that the tag commit is on main and that the
# signing secrets are present; those need repository context, so they stay in
# .github/workflows/release.yml.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

usage() {
  cat >&2 <<'EOF'
usage: scripts/release-preflight.sh <tag> [--latest-tag <tag>] [--allow-existing]

  <tag>             release tag, e.g. v0.2.7 or v0.3.0-rc.1
  --latest-tag      a published release tag; may be repeated. The new tag must
                    be newer than every value passed
  --allow-existing  skip the "newer than latest" check (verification-only rerun
                    of an already published release)
EOF
}

TAG=""
LATEST_TAGS=()
ALLOW_EXISTING=0
while (($# > 0)); do
  case "$1" in
  --latest-tag)
    LATEST_TAG="${2:-}"
    if [[ -z "$LATEST_TAG" ]]; then
      echo "--latest-tag needs a tag" >&2
      exit 2
    fi
    LATEST_TAGS+=("$LATEST_TAG")
    shift 2
    ;;
  --allow-existing)
    ALLOW_EXISTING=1
    shift
    ;;
  -h | --help)
    usage
    exit 0
    ;;
  --*)
    echo "unknown argument: $1" >&2
    usage
    exit 2
    ;;
  *)
    if [[ -n "$TAG" ]]; then
      echo "unexpected argument: $1" >&2
      usage
      exit 2
    fi
    TAG="$1"
    shift
    ;;
  esac
done

if [[ -z "$TAG" ]]; then
  usage
  exit 2
fi

# Semver without leading zeros. The suffix is kept for the changelog lookup and
# for the user-facing version string.
if [[ ! "$TAG" =~ ^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$ ]]; then
  echo "invalid release tag '$TAG' (expected vX.Y.Z or vX.Y.Z-suffix)" >&2
  exit 1
fi
MAJOR="${BASH_REMATCH[1]}"
MINOR="${BASH_REMATCH[2]}"
PATCH="${BASH_REMATCH[3]}"
SUFFIX="${BASH_REMATCH[4]:-}"
VERSION="$MAJOR.$MINOR.$PATCH$SUFFIX"

# Semver numeric prerelease identifiers must not have leading zeros.
if [[ -n "$SUFFIX" ]]; then
  IFS='.' read -r -a PRERELEASE_IDS <<< "${SUFFIX#-}"
  for identifier in "${PRERELEASE_IDS[@]}"; do
    if [[ "$identifier" =~ ^0[0-9]+$ ]]; then
      echo "invalid release tag '$TAG' (numeric prerelease identifier '$identifier' cannot have leading zeros)" >&2
      exit 1
    fi
  done
fi

# The Android version code encodes minor and patch in three digits each.
# Values at or above 1000 would carry into the next component and collide with
# a different release.
if ((MINOR > 999 || PATCH > 999)); then
  echo "version $VERSION cannot be represented as an Android versionCode:" >&2
  echo "minor and patch must be below 1000 (got $MINOR and $PATCH)" >&2
  exit 1
fi

# android-version-code.sh also rejects codes outside the Android range.
bash "$ROOT/scripts/android-version-code.sh" "$VERSION" >/dev/null

python3 - "$ROOT" "$VERSION" <<'PY'
import json
import re
import sys
from pathlib import Path

root = Path(sys.argv[1])
expected = sys.argv[2]
errors = []


def read_text(relative: str) -> str:
    path = root / relative
    try:
        return path.read_text(encoding="utf-8")
    except OSError as error:
        errors.append(f"{relative}: {error}")
        return ""


def read_json(relative: str) -> dict:
    text = read_text(relative)
    if not text:
        return {}
    try:
        return json.loads(text)
    except json.JSONDecodeError as error:
        errors.append(f"{relative}: {error}")
        return {}


def compare(name: str, actual: object) -> None:
    if actual != expected:
        errors.append(f"{name}: {actual!r} (expected {expected!r})")


cargo = read_text("Cargo.toml")
match = re.search(r'^version = "([^"]+)"', cargo, re.MULTILINE)
compare("Cargo.toml", match.group(1) if match else None)

compare("apps/desktop/package.json", read_json("apps/desktop/package.json").get("version"))
compare(
    "apps/desktop/src-tauri/tauri.conf.json",
    read_json("apps/desktop/src-tauri/tauri.conf.json").get("version"),
)
compare("apps/mobile/package.json", read_json("apps/mobile/package.json").get("version"))
mobile_conf = read_json("apps/mobile/src-tauri/tauri.conf.json")
compare("apps/mobile/src-tauri/tauri.conf.json", mobile_conf.get("version"))

core = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)(?:-[0-9A-Za-z.-]+)?", expected)
assert core is not None, expected
expected_code = int(core.group(1)) * 1_000_000 + int(core.group(2)) * 1_000 + int(core.group(3))

android = (mobile_conf.get("bundle") or {}).get("android") or {}
if "versionCode" in android and str(android["versionCode"]) != str(expected_code):
    errors.append(
        f"tauri.conf.json bundle.android.versionCode: {android['versionCode']!r} "
        f"(expected {expected_code})"
    )

# tauri.properties is generated by `tauri android build` and ignored by git, so
# a clean checkout does not have it and a local file may be stale. Report it,
# but never fail: the tracked tauri.conf.json is authoritative, and the Android
# job verifies the built APK's versionName.
properties_path = root / "apps/mobile/src-tauri/gen/android/app/tauri.properties"
if properties_path.exists():
    properties = read_text("apps/mobile/src-tauri/gen/android/app/tauri.properties")
    version_name = re.search(r"^tauri\.android\.versionName=(.+)$", properties, re.MULTILINE)
    version_code = re.search(r"^tauri\.android\.versionCode=(\d+)$", properties, re.MULTILINE)
    stale = []
    if version_name:
        if version_name.group(1).strip() != expected:
            stale.append(
                f"versionName {version_name.group(1).strip()!r} (expected {expected!r})"
            )
    else:
        stale.append("versionName missing")
    if version_code:
        if version_code.group(1) != str(expected_code):
            stale.append(f"versionCode {version_code.group(1)} (expected {expected_code})")
    else:
        stale.append("versionCode missing")
    if stale:
        print(
            "note: ignoring stale generated tauri.properties "
            f"({'; '.join(stale)}); tauri.conf.json is authoritative",
            file=sys.stderr,
        )
else:
    print("note: tauri.properties is generated; using tauri.conf.json versionCode")

if errors:
    print("version source mismatch:", file=sys.stderr)
    for error in errors:
        print(f"  {error}", file=sys.stderr)
    sys.exit(1)
PY

if ! bash "$ROOT/scripts/release-notes.sh" "$TAG" >/dev/null; then
  echo "CHANGELOG.md is not ready for $TAG" >&2
  exit 1
fi

if [[ ${#LATEST_TAGS[@]} -gt 0 && "$ALLOW_EXISTING" -eq 0 ]]; then
  python3 - "$VERSION" "${LATEST_TAGS[@]}" <<'PY'
import re
import sys


def parse(raw: str):
    value = raw.strip()
    if value.startswith("v"):
        value = value[1:]
    match = re.fullmatch(
        r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-([0-9A-Za-z.-]+))?",
        value,
    )
    if not match:
        raise SystemExit(f"invalid version in --latest-tag: {raw!r}")
    core = tuple(int(part) for part in match.group(1, 2, 3))
    return core, match.group(4)


def compare(left: str, right: str) -> int:
    (left_core, left_pre), (right_core, right_pre) = parse(left), parse(right)
    if left_core != right_core:
        return (left_core > right_core) - (left_core < right_core)
    if left_pre is None and right_pre is None:
        return 0
    if left_pre is None:
        return 1
    if right_pre is None:
        return -1

    left_parts = left_pre.split(".")
    right_parts = right_pre.split(".")
    for left_part, right_part in zip(left_parts, right_parts):
        if left_part == right_part:
            continue
        left_numeric = left_part.isdigit()
        right_numeric = right_part.isdigit()
        if left_numeric and right_numeric:
            return (int(left_part) > int(right_part)) - (int(left_part) < int(right_part))
        if left_numeric:
            return -1
        if right_numeric:
            return 1
        return (left_part > right_part) - (left_part < right_part)
    return (len(left_parts) > len(right_parts)) - (len(left_parts) < len(right_parts))


version = sys.argv[1]
for latest in sys.argv[2:]:
    if compare(version, latest) <= 0:
        print(
            f"version {version} must be newer than the latest release {latest}; "
            "bump the version instead of reusing a published one",
            file=sys.stderr,
        )
        sys.exit(1)
PY
fi

echo "release-preflight: OK ($TAG)"
