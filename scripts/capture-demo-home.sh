#!/usr/bin/env bash
# Capture the Live Demo Home view into docs/images/home.png.
# Recaptures when the desktop package version differs from
# docs/images/home.version, or when Live Demo UI files changed.
# CAPTURE_DEMO_HOME_FORCE=1 always recaptures.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

VERSION_FILE="docs/images/home.version"

current_version() {
  node -p "require('./apps/desktop/package.json').version"
}

recorded_version() {
  if [[ -f "$VERSION_FILE" ]]; then
    tr -d '[:space:]' < "$VERSION_FILE"
  fi
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
  reason="screenshot version ${RECORDED_VERSION:-none} -> ${CURRENT_VERSION}"
elif ui_changed || untracked_ui; then
  reason="Live Demo UI changes"
fi

if [[ -z "$reason" ]]; then
  echo "capture-demo-home: skip (screenshot version ${CURRENT_VERSION} is current)"
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

echo "== capture Home screenshot =="
(cd apps/website && node ./scripts/capture-home.mjs)
