#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Quick local gate before committing: fmt, clippy, lib tests, ice-tun-sys
# integration tests, tsc, vitest, and a Live Demo Home screenshot when the
# app version or UI files changed.
# Intentionally lighter than scripts/gate.sh (no desktop vite build, no ice-box crate).
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
# Fast path: unit tests only. CI `scripts/gate.sh` runs the full workspace
# (lib + integration + doc). ice-box is excluded here because it needs
# GTK/webkit, which many local machines lack.
cargo test --workspace --lib --exclude ice-box

echo "== cargo test (ice-tun-sys integration) =="
# Host-free TUN recovery / backend / helper e2e tests live in
# crates/ice-tun-sys/tests/ and are skipped by `--lib`.
cargo test -p ice-tun-sys --tests

echo "== tsc --noEmit =="
(cd apps/desktop && npx tsc --noEmit)

echo "== vitest =="
(cd apps/desktop && npm test)

echo "== merge-updater-latest fixtures =="
bash scripts/test-merge-updater-latest.sh

echo "== capture demo home =="
bash scripts/capture-demo-home.sh

echo "gate-local: OK"