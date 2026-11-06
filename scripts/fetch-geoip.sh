#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Fetch the bundled sing-box rule-sets: GeoIP (Loyalsoldier/geoip, whose
# mainland China ranges come from operator BGP data) and GeoSite
# (SagerNet/sing-geosite). Each source pins a git ref, and its committed
# CHECKSUMS.sha256 is the trust root (CI-8); reviewing a checksums change is
# the trust decision, as with scripts/fetch-singbox.sh.
#
# Both sources land in third_party/bundled-rule-sets/rule-set, the single bundled
# rule-set directory every packaging path copies from. These are the offline
# copies: the app downloads newer or missing sets itself at runtime (sanitized
# before use), and the sing-box core never fetches a rule-set.
#
# Usage: fetch-geoip.sh [--init]
#   --init  write CHECKSUMS.sha256 for every source instead of verifying.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/third_party/bundled-rule-sets/rule-set"

GEOIP_CODES=(cn us jp hk sg kr tw ru gb de fr ca au in nl ch se it es br mx id th vn ph tr ua pl il ae)
# GeoSite categories (v2fly domain-list-community names). Keep this list to
# categories subscriptions commonly route by; each file is a few KB.
GEOSITE_CATEGORIES=(
  cn geolocation-cn 'geolocation-!cn' private category-ads-all
  google youtube github microsoft apple
  telegram twitter facebook tiktok
  netflix disney spotify steam bilibili
  openai anthropic 'category-ai-!cn'
)

INIT=0
if [[ "${1:-}" == "--init" ]]; then
  INIT=1
  shift
fi

if command -v shasum >/dev/null 2>&1; then
  SHA256_CMD="shasum -a 256"
elif command -v sha256sum >/dev/null 2>&1; then
  SHA256_CMD="sha256sum"
else
  echo "no SHA-256 tool (shasum/sha256sum) available" >&2
  exit 1
fi

# fetch_source <repo> <third_party dir> <remote path pattern> <local prefix> <names...>
# The remote path pattern has one %s for the name; files are saved as
# <local prefix>-<name>.srs, the names the config builder looks up.
fetch_source() {
  local repo="$1" dir="$2" pattern="$3" prefix="$4"
  shift 4
  local ref_file="$dir/REF" checksums="$dir/CHECKSUMS.sha256"

  if [[ ! -f "$ref_file" ]]; then
    echo "missing $ref_file (pinned $repo git ref)" >&2
    exit 1
  fi
  local ref
  ref="$(tr -d '[:space:]' <"$ref_file")"
  if [[ -z "$ref" ]]; then
    echo "empty $ref_file" >&2
    exit 1
  fi
  if [[ "$INIT" -eq 0 && ! -f "$checksums" ]]; then
    echo "missing $checksums (expected pinned SHA-256 list); run $0 --init once and commit it" >&2
    exit 1
  fi
  if [[ "$INIT" -eq 1 ]]; then
    : >"$checksums.tmp"
  fi

  local name asset remote url actual expected
  for name in "$@"; do
    asset="${prefix}-${name}.srs"
    # shellcheck disable=SC2059
    remote="$(printf "$pattern" "$name")"
    url="https://raw.githubusercontent.com/${repo}/${ref}/${remote}"
    curl -fsSL -o "$OUT/$asset" "$url"
    # shellcheck disable=SC2086
    actual="$($SHA256_CMD "$OUT/$asset" | awk '{ print $1 }')"
    if [[ "$INIT" -eq 1 ]]; then
      printf '%s  %s\n' "$actual" "$asset" >>"$checksums.tmp"
    else
      expected="$(awk -v asset="$asset" '$2 == asset { print $1; exit }' "$checksums")"
      if [[ -z "$expected" ]]; then
        echo "no SHA-256 entry for $asset in $checksums" >&2
        exit 1
      fi
      if [[ "$actual" != "$expected" ]]; then
        echo "SHA-256 mismatch for $asset" >&2
        echo "  expected: $expected" >&2
        echo "  actual:   $actual" >&2
        exit 1
      fi
    fi
    echo "$asset: ok ($(wc -c <"$OUT/$asset") bytes)"
  done
  if [[ "$INIT" -eq 1 ]]; then
    mv "$checksums.tmp" "$checksums"
    echo "wrote $checksums"
  fi
  echo "fetched $# rule-sets from ${repo}@${ref} into $OUT"
}

cleanup() {
  rm -f "$ROOT"/third_party/loyalsoldier-geoip/CHECKSUMS.sha256.tmp \
    "$ROOT"/third_party/sing-geosite/CHECKSUMS.sha256.tmp
}
trap cleanup EXIT

mkdir -p "$OUT"
fetch_source Loyalsoldier/geoip "$ROOT/third_party/loyalsoldier-geoip" 'srs/%s.srs' geoip "${GEOIP_CODES[@]}"
fetch_source SagerNet/sing-geosite "$ROOT/third_party/sing-geosite" 'geosite-%s.srs' geosite "${GEOSITE_CATEGORIES[@]}"
