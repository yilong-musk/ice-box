#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Workspace gate (plan G9.10): fmt, clippy, tests, frontend typecheck.
#
# GATE_SCOPE picks which half runs:
#   all      (default) both halves — local runs and the Linux CI gate
#   rust     fmt + clippy + cargo test
#   frontend tsc + vitest + fixtures + vite build
# The macOS/Windows CI test jobs use `rust`: the frontend half is platform
# independent and already covered by the Linux gate, and `tauri build`
# (in the parallel packaging jobs) re-runs `npm run build` anyway.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

SCOPE="${GATE_SCOPE:-all}"
case "$SCOPE" in
all | rust | frontend) ;;
*)
  echo "unknown GATE_SCOPE: $SCOPE (expected all | rust | frontend)" >&2
  exit 2
  ;;
esac

if [[ "$SCOPE" == all || "$SCOPE" == rust ]]; then
  echo "== cargo fmt --check =="
  cargo fmt --check

  echo "== cargo clippy =="
  cargo clippy --workspace --all-targets -- -D warnings

  echo "== cargo test (workspace: lib + integration + doc) =="
  # ice-box is the Tauri shell (cdylib, no crate integration tests). Test it
  # with `--lib`; other crates still run lib + integration + doc tests.
  # Windows lib-harness load (ComCtl32 v6) is handled in desktop build.rs.
  cargo test --workspace --exclude ice-box

  echo "== cargo test (ice-box lib) =="
  cargo test -p ice-box --lib
fi

if [[ "$SCOPE" == all || "$SCOPE" == frontend ]]; then
  echo "== tsc --noEmit =="
  # `npx tsc` resolves the stub npm package `tsc`, not `typescript`.
  (cd apps/desktop && npm run typecheck)
  (cd apps/website && npm run typecheck)
  (cd apps/mobile && npm run typecheck)

  echo "== website installer links =="
  (cd apps/website && npm test)

  echo "== website build =="
  (cd apps/website && npm run build)

  echo "== mobile live demo smoke =="
  # Builds are already in place; drives the phone iframe: node-list warning,
  # tabs, apps fixtures, per-app routing, VPN power toggle, language and theme.
  (cd apps/website && npx playwright install chromium && npm run test:mobile-demo)

  echo "== vitest =="
  (cd apps/desktop && npm test)

  echo "== merge-updater-latest fixtures =="
  bash "$ROOT/scripts/test-merge-updater-latest.sh"

  echo "== release preflight fixtures =="
  bash "$ROOT/scripts/test-release-preflight.sh"

  echo "== updater signature fixtures =="
  bash "$ROOT/scripts/test-verify-updater-signatures.sh"

  echo "== signing key fixtures =="
  bash "$ROOT/scripts/test-verify-signing-key.sh"

  echo "== release asset fixtures =="
  bash "$ROOT/scripts/test-verify-release-assets.sh"

  echo "== release state fixtures =="
  bash "$ROOT/scripts/test-gh-release-state.sh"

  echo "== android keystore fixtures =="
  bash "$ROOT/scripts/test-verify-android-keystore.sh"

  echo "== android version code =="
  bash "$ROOT/scripts/test-android-version-code.sh"

  echo "== android apk signer lines =="
  bash "$ROOT/scripts/test-verify-android-apk.sh"

  echo "== vite build =="
  (cd apps/desktop && npm run build)
  # Documentation screenshots are refreshed separately by
  # scripts/capture-docs.sh; the gate does not write tracked files.
fi

echo "G9.10 gate ($SCOPE): OK"
