#!/usr/bin/env bash
# Quick local gate before committing: fmt, clippy, lib tests, tsc, vitest.
# Intentionally lighter than scripts/gate.sh (no vite build, no desktop deps).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "== cargo fmt --check =="
cargo fmt --check

echo "== cargo clippy =="
# Exclude the Tauri desktop crate: it needs GTK/webkit system libs that are
# installed on CI runners but often missing on dev machines. CI gate.sh covers it.
cargo clippy --workspace --all-targets --exclude ice-box -- -D warnings

echo "== cargo test (lib) =="
cargo test --workspace --lib --exclude ice-box

echo "== tsc --noEmit =="
(cd apps/desktop && npx tsc --noEmit)

echo "== vitest =="
(cd apps/desktop && npm test)

echo "gate-local: OK"