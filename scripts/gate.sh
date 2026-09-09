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

  echo "== vitest =="
  (cd apps/desktop && npm test)

  echo "== merge-updater-latest fixtures =="
  bash "$ROOT/scripts/test-merge-updater-latest.sh"

  echo "== vite build =="
  (cd apps/desktop && npm run build)

  if [[ -z "${CI:-}" ]]; then
    echo "== capture demo home =="
    bash "$ROOT/scripts/capture-demo-home.sh"
  fi
fi

echo "G9.10 gate ($SCOPE): OK"
