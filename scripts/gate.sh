#!/usr/bin/env bash
# Workspace gate (plan G9.10): fmt, clippy, tests, frontend typecheck.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "== cargo fmt --check =="
cargo fmt --check

echo "== cargo clippy =="
cargo clippy --workspace --all-targets -- -D warnings

echo "== cargo test (lib) =="
cargo test --workspace --lib

echo "== tsc --noEmit =="
cd apps/desktop
npx tsc --noEmit

echo "== vitest =="
npm test

echo "== vite build =="
npm run build

if [[ -z "${CI:-}" ]]; then
  echo "== capture demo home =="
  bash "$ROOT/scripts/capture-demo-home.sh"
fi

echo "G9.10 gate: OK"
