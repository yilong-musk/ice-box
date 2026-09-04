#!/usr/bin/env bash
# Workspace gate (plan G9.10): fmt, clippy, tests, frontend typecheck.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# Windows only: the tauri build script validates the declared bundle
# resources at compile time; the TUN task launcher (plan B) must be present
# in resources/ for the gate's cargo build to pass. The tauri beforeBuild
# step rebuilds + copies it again for the real bundle.
if [[ "${OSTYPE:-}" == msys* || "${OSTYPE:-}" == cygwin* ]]; then
  echo "== build + stage ice-tun-launcher (Windows) =="
  cargo build --release -p ice-tun-launcher
  cp target/release/ice-tun-launcher.exe apps/desktop/src-tauri/resources/
fi

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

echo "G9.10 gate: OK"
