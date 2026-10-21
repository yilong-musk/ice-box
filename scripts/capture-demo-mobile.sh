#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Capture the phone Home view in a current-generation Pixel-style shell as a
# local source for the composed README product shot (hero*.png):
#   docs/images/mobile-home.png      mobile-home.zh-CN.png
# These captures are ignored by git.
# Recaptures when the mobile package version differs from
# docs/images/mobile.version, when phone UI files changed, or when the
# screenshot is missing.
# CAPTURE_DEMO_MOBILE_FORCE=1 always recaptures.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

VERSION_FILE="docs/images/mobile.version"
SCREENSHOTS=(
  "docs/images/mobile-home.png"
  "docs/images/mobile-home.zh-CN.png"
)

current_version() {
  node -p "require('./apps/mobile/package.json').version"
}

recorded_version() {
  if [[ -f "$VERSION_FILE" ]]; then
    tr -d '[:space:]' < "$VERSION_FILE"
  fi
}

screenshots_missing() {
  local file
  for file in "${SCREENSHOTS[@]}"; do
    if [[ ! -f "$file" ]]; then
      return 0
    fi
  done
  return 1
}

ui_changed() {
  git diff --name-only HEAD -- \
    apps/desktop/src apps/mobile/src apps/mobile/capture.html \
    apps/website/src/browser-api.ts
}

untracked_ui() {
  git ls-files --others --exclude-standard -- \
    apps/desktop/src apps/mobile/src apps/mobile/capture.html \
    apps/website/src/browser-api.ts
}

CURRENT_VERSION="$(current_version)"
RECORDED_VERSION="$(recorded_version)"

reason=""
if [[ "${CAPTURE_DEMO_MOBILE_FORCE:-}" == "1" ]]; then
  reason="forced"
elif [[ "$CURRENT_VERSION" != "$RECORDED_VERSION" ]]; then
  reason="screenshots version ${RECORDED_VERSION:-none} -> ${CURRENT_VERSION}"
elif screenshots_missing; then
  reason="missing screenshots"
elif [[ -n "$(ui_changed)" ]]; then
  reason="phone UI changes"
elif [[ -n "$(untracked_ui)" ]]; then
  reason="new phone UI files"
fi

if [[ -z "$reason" ]]; then
  echo "capture-demo-mobile: skip (screenshots are current for version ${CURRENT_VERSION})"
  exit 0
fi

echo "capture-demo-mobile: recapture (${reason})"

if [[ ! -d apps/website/node_modules/playwright ]]; then
  echo "capture-demo-mobile: install website deps first:" >&2
  echo "  (cd apps/website && npm ci && npx playwright install chromium)" >&2
  exit 1
fi
if [[ ! -x apps/mobile/node_modules/.bin/vite ]]; then
  echo "capture-demo-mobile: install desktop deps first (mobile links them):" >&2
  echo "  (cd apps/desktop && npm ci)" >&2
  exit 1
fi

echo "== playwright chromium =="
(cd apps/website && npx playwright install chromium)

echo "== capture phone screenshots =="
(cd apps/website && node ./scripts/capture-mobile.mjs)
