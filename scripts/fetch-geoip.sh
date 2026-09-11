#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Fetch sing-box GeoIP rule-sets (sing-geoip). The committed CHECKSUMS.sha256
# file is the trust root (CI-8); reviewing a checksums change is the trust
# decision, as with scripts/fetch-singbox.sh.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/third_party/sing-geoip/rule-set"
REF_FILE="$ROOT/third_party/sing-geoip/REF"
CHECKSUMS="$ROOT/third_party/sing-geoip/CHECKSUMS.sha256"
CODES=(cn us jp hk sg kr tw ru gb de fr ca au in nl ch se it es br mx id th vn ph tr ua pl il ae)

if [[ ! -f "$REF_FILE" ]]; then
  echo "missing $REF_FILE (pinned SagerNet/sing-geoip git ref)" >&2
  exit 1
fi
REF="$(tr -d '[:space:]' <"$REF_FILE")"
if [[ -z "$REF" ]]; then
  echo "empty $REF_FILE" >&2
  exit 1
fi

INIT=0
if [[ "${1:-}" == "--init" ]]; then
  INIT=1
  shift
fi

if [[ "$INIT" -eq 0 && ! -f "$CHECKSUMS" ]]; then
  echo "missing $CHECKSUMS (expected pinned SHA-256 list); run $0 --init once and commit it" >&2
  exit 1
fi

if command -v shasum >/dev/null 2>&1; then
  SHA256_CMD="shasum -a 256"
elif command -v sha256sum >/dev/null 2>&1; then
  SHA256_CMD="sha256sum"
else
  echo "no SHA-256 tool (shasum/sha256sum) available" >&2
  exit 1
fi

mkdir -p "$OUT"
if [[ "$INIT" -eq 1 ]]; then
  : >"$CHECKSUMS.tmp"
  trap 'rm -f "$CHECKSUMS.tmp"' EXIT
fi
for code in "${CODES[@]}"; do
  asset="geoip-${code}.srs"
  url="https://raw.githubusercontent.com/SagerNet/sing-geoip/${REF}/${asset}"
  curl -fsSL -o "$OUT/$asset" "$url"
  # shellcheck disable=SC2086
  actual="$($SHA256_CMD "$OUT/$asset" | awk '{ print $1 }')"
  if [[ "$INIT" -eq 1 ]]; then
    printf '%s  %s\n' "$actual" "$asset" >>"$CHECKSUMS.tmp"
  else
    expected="$(awk -v asset="$asset" '$2 == asset { print $1; exit }' "$CHECKSUMS")"
    if [[ -z "$expected" ]]; then
      echo "no SHA-256 entry for $asset in $CHECKSUMS" >&2
      exit 1
    fi
    if [[ "$actual" != "$expected" ]]; then
      echo "SHA-256 mismatch for $asset" >&2
      echo "  expected: $expected" >&2
      echo "  actual:   $actual" >&2
      exit 1
    fi
  fi
  echo "$asset: ok ($(wc -c < "$OUT/$asset") bytes)"
done
if [[ "$INIT" -eq 1 ]]; then
  mv "$CHECKSUMS.tmp" "$CHECKSUMS"
  trap - EXIT
  echo "wrote $CHECKSUMS"
fi
echo "fetched ${#CODES[@]} rule-sets from sing-geoip@${REF} into $OUT"
