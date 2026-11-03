#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Local gate before committing. Picks the checks that match the files changed
# on this branch (committed since the merge base with main, staged, unstaged,
# and untracked), so a Rust-only change does not build the website or run the
# release-script fixtures.
#
#   bash scripts/gate-local.sh          scoped run (default)
#   bash scripts/gate-local.sh --full   every check, regardless of changes
#   bash scripts/gate-local.sh --dry-run  print the selected scope and exit
#
# Buckets:
#   rust      fmt, clippy, lib tests, ice-tun-sys integration tests
#   frontend  desktop/website/mobile typecheck, website installer links,
#             mobile Live Demo smoke, Vitest
#   fixtures  release, updater, signing, and Android script fixtures
# `cargo fmt --check` runs on every invocation. Docs-only changes run nothing
# else. A path that matches no bucket, a change to this script, or a missing
# merge base falls back to --full.
#
# GATE_LOCAL_BASE overrides the base ref (default: origin/main, then main).
# Intentionally lighter than scripts/gate.sh (no desktop vite build, no
# Tauri shell crates). Documentation screenshots are not part of the gate;
# refresh them with scripts/capture-docs.sh.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

full=0
dry_run=0
for arg in "$@"; do
  case "$arg" in
  --full) full=1 ;;
  --dry-run) dry_run=1 ;;
  -h | --help)
    sed -n '3,24p' "$0" | sed 's/^# \{0,1\}//'
    exit 0
    ;;
  *)
    echo "gate-local: unknown argument: $arg (expected --full or --dry-run)" >&2
    exit 2
    ;;
  esac
done

run_rust=0
run_frontend=0
run_fixtures=0

# Classify one repository-relative path into the buckets above. Returns 1 for
# a path that matches no known area so the caller can fall back to --full.
classify() {
  local path="$1"
  case "$path" in
  scripts/gate-local.sh)
    return 1
    ;;
  *.md | docs/* | LICENSE | NOTICE | .gitignore | .vscode/* | .github/*)
    ;;
  apps/desktop/src-tauri/* | apps/mobile/src-tauri/* | apps/mobile/plugins/*)
    run_rust=1
    ;;
  crates/* | configs/* | third_party/* | spikes/* | Cargo.toml | Cargo.lock | \
    rust-toolchain.toml | deny.toml | .cargo/*)
    run_rust=1
    ;;
  apps/desktop/* | apps/mobile/* | apps/website/* | package.json | package-lock.json)
    run_frontend=1
    ;;
  scripts/*)
    run_fixtures=1
    ;;
  *)
    return 1
    ;;
  esac
}

base=""
if [[ "$full" == 0 ]]; then
  for ref in "${GATE_LOCAL_BASE:-}" origin/main main; do
    [[ -n "$ref" ]] || continue
    if base="$(git merge-base HEAD "$ref" 2>/dev/null)"; then
      break
    fi
    base=""
  done
  if [[ -z "$base" ]]; then
    echo "gate-local: no merge base with main; running every check"
    full=1
  fi
fi

if [[ "$full" == 0 ]]; then
  changed="$(
    {
      git diff --name-only "$base"
      git ls-files --others --exclude-standard
    } | sort -u
  )"
  while IFS= read -r path; do
    [[ -n "$path" ]] || continue
    if ! classify "$path"; then
      echo "gate-local: $path is outside the scoped buckets; running every check"
      full=1
      break
    fi
  done <<<"$changed"
fi

if [[ "$full" == 1 ]]; then
  run_rust=1
  run_frontend=1
  run_fixtures=1
fi

scope=()
[[ "$run_rust" == 1 ]] && scope+=(rust)
[[ "$run_frontend" == 1 ]] && scope+=(frontend)
[[ "$run_fixtures" == 1 ]] && scope+=(fixtures)
echo "gate-local: scope = ${scope[*]:-fmt only}$([[ "$full" == 1 ]] && echo ' (full)')"
[[ "$dry_run" == 1 ]] && exit 0

echo "== cargo fmt --check =="
cargo fmt --check

if [[ "$run_rust" == 1 ]]; then
  echo "== cargo clippy =="
  # Exclude the Tauri shells: they need GTK/webkit system libs that are
  # installed on CI runners but often missing on dev machines. CI gate.sh covers them.
  cargo clippy --workspace --all-targets --exclude ice-box --exclude ice-box-mobile -- -D warnings

  echo "== cargo test (lib) =="
  # Fast path: unit tests only. CI `scripts/gate.sh` runs lib + integration +
  # doc tests for every crate except ice-box, then `cargo test -p ice-box --lib`.
  # The Tauri shells are excluded here because they need GTK/webkit, which many
  # local machines lack.
  cargo test --workspace --lib --exclude ice-box --exclude ice-box-mobile

  echo "== cargo test (ice-tun-sys integration) =="
  # Host-free TUN recovery / backend / helper e2e tests live in
  # crates/ice-tun-sys/tests/ and are skipped by `--lib`.
  cargo test -p ice-tun-sys --tests
fi

if [[ "$run_frontend" == 1 ]]; then
  echo "== tsc --noEmit =="
  # `npx tsc` resolves the stub npm package `tsc`, not `typescript`.
  # The three apps share sources (mobile and website import desktop UI), so
  # any frontend change typechecks all of them.
  (cd apps/desktop && npm run typecheck)
  (cd apps/website && npm run typecheck)
  (cd apps/mobile && npm run typecheck)

  echo "== website installer links =="
  (cd apps/website && npm test)

  echo "== mobile live demo smoke =="
  # Builds the site, then drives the phone iframe: tabs, apps fixtures, VPN
  # power toggle, and the landing-page language bridge.
  (cd apps/website && npm run build && npx playwright install chromium && npm run test:mobile-demo)

  echo "== vitest =="
  (cd apps/desktop && npm test)
fi

if [[ "$run_fixtures" == 1 ]]; then
  echo "== merge-updater-latest fixtures =="
  bash scripts/test-merge-updater-latest.sh

  echo "== release preflight fixtures =="
  bash scripts/test-release-preflight.sh

  echo "== updater signature fixtures =="
  bash scripts/test-verify-updater-signatures.sh

  echo "== signing key fixtures =="
  bash scripts/test-verify-signing-key.sh

  echo "== release asset fixtures =="
  bash scripts/test-verify-release-assets.sh

  echo "== release state fixtures =="
  bash scripts/test-gh-release-state.sh

  echo "== android keystore fixtures =="
  bash scripts/test-verify-android-keystore.sh

  echo "== android version code =="
  bash scripts/test-android-version-code.sh

  echo "== android apk signer lines =="
  bash scripts/test-verify-android-apk.sh
fi

echo "gate-local: OK"
