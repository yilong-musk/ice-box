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
#   rust      fmt, clippy, lib tests, ice-tun-sys integration tests; plus
#             clippy and lib tests for a changed Tauri shell when its GTK /
#             WebKit build dependencies are available
#   frontend  desktop/website/mobile typecheck, website installer links,
#             mobile Live Demo smoke, Vitest
#   vitest    Vitest alone, for Rust files the architecture tests read
#   fixtures  release, updater, signing, and Android script fixtures, plus
#             `bash -n` on changed shell scripts; also selected by the
#             version sources and CHANGELOG.md that release preflight reads
# `cargo fmt --check`, the Rust toolchain pin check, and the documentation
# version check run on every invocation. Docs-only changes run nothing else.
# A path that matches no bucket, a change to this script, or a missing merge
# base falls back to --full.
#
# GATE_LOCAL_BASE overrides the base ref (default: origin/main, then main).
# Intentionally lighter than scripts/gate.sh (no desktop vite build, and the
# Tauri shell crates only when they changed). Documentation screenshots are not
# part of the gate; refresh them with scripts/capture-docs.sh.
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
    sed -n '3,30p' "$0" | sed 's/^# \{0,1\}//'
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
run_vitest=0
run_fixtures=0
# Tauri shell crates (ice-box, ice-box-mobile) with changed sources.
shells=()
# Changed shell scripts, syntax-checked in the fixtures bucket.
changed_scripts=()

# Classify one repository-relative path into the buckets above. Returns 1 for
# a path that matches no known area so the caller can fall back to --full.
classify() {
  local path="$1"

  # Cross-bucket inputs: files one bucket owns but another bucket's checks
  # read. They add to the primary bucket chosen below.
  case "$path" in
  # apps/desktop/src/api/architecture.test.ts (Vitest) scans these.
  apps/desktop/src-tauri/src/application/* | crates/ice-app/Cargo.toml)
    run_vitest=1
    ;;
  esac
  case "$path" in
  # The release preflight fixture checks the real repo: every version source
  # and the CHANGELOG.md section for the current version.
  CHANGELOG.md | Cargo.toml | apps/desktop/package.json | apps/mobile/package.json | \
    apps/desktop/src-tauri/tauri.conf.json | apps/mobile/src-tauri/tauri.conf.json)
    run_fixtures=1
    ;;
  esac

  case "$path" in
  scripts/gate-local.sh)
    return 1
    ;;
  *.md | docs/* | LICENSE | NOTICE | .gitignore | .vscode/* | .github/*)
    ;;
  apps/desktop/src-tauri/*)
    run_rust=1
    add_shell ice-box
    ;;
  apps/mobile/src-tauri/* | apps/mobile/plugins/*)
    run_rust=1
    add_shell ice-box-mobile
    ;;
  crates/* | configs/* | third_party/* | spikes/* | Cargo.toml | Cargo.lock | \
    rust-toolchain.toml | deny.toml | .cargo/*)
    run_rust=1
    ;;
  apps/desktop/* | apps/mobile/* | apps/website/* | package.json | package-lock.json)
    run_frontend=1
    ;;
  scripts/*)
    # The fixtures exercise the release, updater, signing, and Android
    # scripts. Build helpers (fetch-singbox, prepare-singbox-resource, ...)
    # only run inside `tauri build`, which this gate never does; they get a
    # syntax check here and real coverage from CI's scripts/gate.sh.
    run_fixtures=1
    if [[ "$path" == *.sh && -f "$path" ]]; then
      changed_scripts+=("$path")
    fi
    ;;
  *)
    return 1
    ;;
  esac
}

add_shell() {
  local crate
  # `${shells[@]+...}` keeps the first call (empty array) from tripping
  # `set -u` on bash 3.2 (macOS /bin/bash); bash 4.4+ on Linux accepts both.
  for crate in ${shells[@]+"${shells[@]}"}; do
    [[ "$crate" == "$1" ]] && return 0
  done
  shells+=("$1")
}

# The Tauri shells link GTK/WebKit on Linux; macOS and Windows build them with
# the system web view.
shell_deps_available() {
  [[ "$(uname -s)" != Linux ]] && return 0
  command -v pkg-config >/dev/null 2>&1 &&
    pkg-config --exists webkit2gtk-4.1 javascriptcoregtk-4.1 libsoup-3.0
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
      # --no-renames lists both sides of a rename, so a file moved out of a
      # code area still selects the checks for that area.
      git diff --name-only --no-renames "$base"
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
  shells=(ice-box ice-box-mobile)
fi
# The frontend bucket already runs Vitest.
[[ "$run_frontend" == 1 ]] && run_vitest=0

shells_runnable=0
if [[ "${#shells[@]}" -gt 0 ]] && shell_deps_available; then
  shells_runnable=1
fi

scope=()
[[ "$run_rust" == 1 ]] && scope+=(rust)
if [[ "${#shells[@]}" -gt 0 ]]; then
  if [[ "$shells_runnable" == 1 ]]; then
    scope+=("shells(${shells[*]})")
  else
    scope+=("shells(${shells[*]}: skipped, GTK/WebKit missing)")
  fi
fi
[[ "$run_frontend" == 1 ]] && scope+=(frontend)
[[ "$run_vitest" == 1 ]] && scope+=(vitest)
[[ "$run_fixtures" == 1 ]] && scope+=(fixtures)
# Assigned in two statements: bash 3.2 (macOS /bin/bash) cannot parse a
# command substitution nested inside a parameter expansion inside quotes.
scope_label="${scope[*]:-fmt, toolchain pins, doc versions only}"
[[ "$full" == 1 ]] && scope_label+=" (full)"
echo "gate-local: scope = $scope_label"
[[ "$dry_run" == 1 ]] && exit 0

echo "== cargo fmt --check =="
cargo fmt --check

echo "== rust toolchain pins =="
bash scripts/check-toolchain-pins.sh

echo "== documentation versions =="
bash scripts/check-doc-versions.sh

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

if [[ "${#shells[@]}" -gt 0 ]]; then
  if [[ "$shells_runnable" == 1 ]]; then
    shell_args=()
    for crate in "${shells[@]}"; do
      shell_args+=(-p "$crate")
    done
    echo "== cargo clippy (Tauri shells: ${shells[*]}) =="
    cargo clippy "${shell_args[@]}" --all-targets -- -D warnings

    echo "== cargo test (Tauri shells lib: ${shells[*]}) =="
    cargo test "${shell_args[@]}" --lib
  else
    echo "gate-local: WARNING: ${shells[*]} changed but GTK/WebKit dev packages are missing;"
    echo "gate-local: WARNING: these crates are not compiled here. CI scripts/gate.sh covers them."
  fi
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

if [[ "$run_vitest" == 1 ]]; then
  echo "== vitest (architecture inputs changed) =="
  (cd apps/desktop && npm test)
fi

if [[ "$run_fixtures" == 1 ]]; then
  if [[ "${#changed_scripts[@]}" -gt 0 ]]; then
    echo "== bash -n (changed scripts) =="
    for script in "${changed_scripts[@]}"; do
      bash -n "$script"
    done
  fi

  echo "== merge-updater-latest fixtures =="
  bash scripts/test-merge-updater-latest.sh

  echo "== toolchain pin fixtures =="
  bash scripts/test-check-toolchain-pins.sh

  echo "== documentation version fixtures =="
  bash scripts/test-check-doc-versions.sh

  echo "== changelog release fixtures =="
  bash scripts/test-changelog-release.sh

  echo "== CI status fixtures =="
  bash scripts/test-verify-ci-status.sh

  echo "== release environment fixtures =="
  bash scripts/test-verify-release-environment.sh

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
