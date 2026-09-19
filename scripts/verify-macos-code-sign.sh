#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Verify that a built .app carries a complete ad-hoc code signature.
# Usage: scripts/verify-macos-code-sign.sh <path-to-.app>
#
# The macOS release has no Developer ID certificate and is not notarized
# (see docs/release-process.md), so the bundle must be ad-hoc signed end to
# end. Without a bundle-level seal Gatekeeper reports the app as "damaged";
# a complete ad-hoc signature downgrades the prompt to "unidentified
# developer", which the user can override (right-click Open / System
# Settings "Open Anyway").
#
# Assertions:
#   1. `codesign --verify --strict --verbose=2` exits 0. Strict mode
#      reproduces the Gatekeeper/spctl integrity check (Apple TN2206), so a
#      stale or missing seal fails here.
#   2. `codesign -dvv` output contains the ad-hoc marker (e.g.
#      `flags=0x10002(runtime,adhoc)` or `Signature=adhoc`).
#   3. `Contents/_CodeSignature/CodeResources` exists.
#
# `spctl --assess` still rejects ad-hoc bundles (its policy requires a
# Developer ID, not just any signature) and is deliberately NOT asserted.
#
# macOS only: `codesign` ships with the Xcode command line tools.
set -euo pipefail

fail() {
  echo "verify-macos-code-sign: $*" >&2
  exit 1
}

APP="${1:-}"
# Accept both `Foo.app` and `Foo.app/`.
APP="${APP%/}"

[[ -n "$APP" ]] || fail "usage: $0 <path-to-.app>"
[[ "$APP" == *.app ]] || fail "not a .app bundle path: $APP"
[[ -d "$APP" ]] || fail "bundle directory not found: $APP"
command -v codesign >/dev/null 2>&1 ||
  fail "codesign not found; run this on macOS with the Xcode command line tools"

echo "verifying code signature: $APP"

# 1. Strict verification: the same integrity check Gatekeeper performs for
#    bundles. A missing or stale seal exits non-zero with the reason on stderr.
codesign --verify --strict --verbose=2 "$APP"

# 2. The bundle signature itself must be ad-hoc (this project ships without a
#    Developer ID certificate).
DETAILS="$(codesign -dvv "$APP" 2>&1 || true)"
if ! grep -q 'adhoc' <<<"$DETAILS"; then
  echo "$DETAILS" >&2
  fail "bundle is not ad-hoc signed"
fi

# 3. The seal catalog file must exist inside the bundle.
SEAL="$APP/Contents/_CodeSignature/CodeResources"
[[ -f "$SEAL" ]] || fail "bundle seal file missing: $SEAL"

echo "ok: ad-hoc code signature verified; seal: $SEAL"
