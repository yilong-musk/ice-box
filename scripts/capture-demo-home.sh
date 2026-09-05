#!/usr/bin/env bash
# Capture the Live Demo Home view into docs/images/home.png.
# Skips when the working tree has no desktop/website UI changes unless
# CAPTURE_DEMO_HOME_FORCE=1.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

ui_changed() {
  [[ -n "$(git diff --name-only HEAD -- apps/desktop/src apps/website)" ]]
}

untracked_ui() {
  [[ -n "$(git ls-files --others --exclude-standard -- apps/desktop/src apps/website)" ]]
}

if [[ "${CAPTURE_DEMO_HOME_FORCE:-}" != "1" ]]; then
  if ! ui_changed && ! untracked_ui; then
    echo "capture-demo-home: skip (no Live Demo UI changes)"
    exit 0
  fi
fi

if [[ ! -d apps/website/node_modules/playwright ]]; then
  echo "capture-demo-home: install website deps first:" >&2
  echo "  (cd apps/website && npm ci && npx playwright install chromium)" >&2
  exit 1
fi

echo "== playwright chromium =="
(cd apps/website && npx playwright install chromium)

echo "== capture Home screenshot =="
(cd apps/website && node ./scripts/capture-home.mjs)
