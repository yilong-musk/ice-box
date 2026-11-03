#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Refresh the documentation screenshots and recompose the README hero.
#
# The desktop and Android Home captures are retaken only when the app version
# or the relevant UI files changed, or a capture is missing (see
# capture-demo-home.sh / capture-demo-mobile.sh). The hero is recomposed on
# every run. Run this when the Live Demo UI changes and before a release; the
# gates do not run it. Commit only docs/images/hero.png and
# docs/images/hero.zh-CN.png.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "== capture demo home =="
bash scripts/capture-demo-home.sh

echo "== capture demo mobile =="
bash scripts/capture-demo-mobile.sh

echo "== compose README hero =="
(cd apps/website && npm run capture-hero)

echo "capture-docs: OK"
