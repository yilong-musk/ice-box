#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Capture the Live Demo Home view into docs/images/home.png (English) and
# docs/images/home.zh-CN.png (Chinese).
# Recaptures when the desktop package version differs from
# docs/images/home.version, when Live Demo UI files changed, or when either
# screenshot is missing.
# CAPTURE_DEMO_HOME_FORCE=1 always recaptures.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

VERSION_FILE="docs/images/home.version"
SCREENSHOTS=("docs/images/home.png" "docs/images/home.zh-CN.png")

current_version() {
  node -p "require('./apps/desktop/package.json').version"
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
  [[ -n "$(git diff --name-only HEAD -- apps/desktop/src apps/website)" ]]
}

untracked_ui() {
  [[ -n "$(git ls-files --others --exclude-standard -- apps/desktop/src apps/website)" ]]
}

CURRENT_VERSION="$(current_version)"
RECORDED_VERSION="$(recorded_version)"

reason=""
if [[ "${CAPTURE_DEMO_HOME_FORCE:-}" == "1" ]]; then
  reason="forced"
elif [[ "$CURRENT_VERSION" != "$RECORDED_VERSION" ]]; then
  reason="screenshots version ${RECORDED_VERSION:-none} -> ${CURRENT_VERSION}"
elif screenshots_missing; then
  reason="missing screenshots"
elif ui_changed || untracked_ui; then
  reason="Live Demo UI changes"
fi

if [[ -z "$reason" ]]; then
  echo "capture-demo-home: skip (screenshots are current for version ${CURRENT_VERSION})"
  exit 0
fi

echo "capture-demo-home: recapture (${reason})"

if [[ ! -d apps/website/node_modules/playwright ]]; then
  echo "capture-demo-home: install website deps first:" >&2
  echo "  (cd apps/website && npm ci && npx playwright install chromium)" >&2
  exit 1
fi

echo "== playwright chromium =="
(cd apps/website && npx playwright install chromium)

echo "== capture Home screenshots =="
(cd apps/website && node ./scripts/capture-home.mjs)
