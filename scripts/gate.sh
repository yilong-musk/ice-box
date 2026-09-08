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
cargo test --workspace

echo "== tsc --noEmit =="
(cd apps/desktop && npx tsc --noEmit)
(cd apps/website && npx tsc --noEmit)
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
