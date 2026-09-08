#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Workspace gate (plan G9.10): fmt, clippy, tests, frontend typecheck.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

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

echo "== tsc --noEmit =="
# `npx tsc` resolves the stub npm package `tsc`, not `typescript`.
(cd apps/desktop && npm run typecheck)
(cd apps/website && npm run typecheck)
cd apps/desktop

echo "== vitest =="
npm test

echo "== merge-updater-latest fixtures =="
bash "$ROOT/scripts/test-merge-updater-latest.sh"

echo "== vite build =="
npm run build

if [[ -z "${CI:-}" ]]; then
  echo "== capture demo home =="
  bash "$ROOT/scripts/capture-demo-home.sh"
fi

echo "G9.10 gate: OK"
