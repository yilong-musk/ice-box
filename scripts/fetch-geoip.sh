#!/usr/bin/env bash
# Fetch sing-box GeoIP rule-sets (sing-geoip, MIT-ish license, see repo LICENSE).
# The binary .srs files are committed so builds stay offline.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/third_party/sing-geoip/rule-set"
CODES=(cn us jp hk sg kr tw ru gb de fr ca au in nl ch se it es br mx id th vn ph tr ua pl il ae)

mkdir -p "$OUT"
for code in "${CODES[@]}"; do
  url="https://raw.githubusercontent.com/SagerNet/sing-geoip/rule-set/geoip-${code}.srs"
  curl -fsSL -o "$OUT/geoip-${code}.srs" "$url"
  echo "geoip-${code}.srs: $(wc -c < "$OUT/geoip-${code}.srs") bytes"
done
echo "fetched ${#CODES[@]} rule-sets into $OUT"
