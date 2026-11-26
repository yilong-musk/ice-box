#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Local gate before committing. Picks the narrowest safe checks that match the
# files you changed and runs independent buckets in parallel.
#
#   bash scripts/gate-local.sh             uncommitted changes vs HEAD (or branch
#                                          vs main when the working tree is clean)
#   bash scripts/gate-local.sh --branch    every change on this branch vs main
#   bash scripts/gate-local.sh --full      every check, regardless of changes
#   bash scripts/gate-local.sh --dry-run   print the selected scope and exit
#   bash scripts/gate-local.sh --serial    run checks sequentially with live output
#
# Diff selection:
#   - Default: when the working tree has staged, unstaged, or untracked changes,
#     scope to those uncommitted files relative to HEAD so follow-up commits on
#     a large branch do not re-run unrelated buckets. When the working tree is
#     clean (for example before pushing or opening a PR), scope to the full
#     branch relative to merge-base(HEAD, main).
#   - `--branch` (or setting GATE_LOCAL_BASE) always scopes to the full branch.
#   - `--full` runs every check (required before a formal release).
#
# Buckets (run in parallel when multiple are selected):
#   rust      crate-scoped clippy (--all-targets) and tests for changed Rust
#             crates and their transitive non-shell workspace dependents, plus
#             `ice-tun-sys` integration tests when `ice-tun-sys` is affected,
#             and Tauri shell crates (`ice-box`, `ice-box-mobile`) when their
#             sources changed and GTK/WebKit dev packages are available
#   frontend  split into typecheck (desktop/website/mobile in parallel),
#             desktop Vitest, fast architecture/version Vitest for cross-app
#             boundary inputs, website installer-link tests, and website build
#             + mobile Live Demo smoke (reusing cached Playwright Chromium)
#   fixtures  `bash -n` on changed scripts plus all release/updater/signing/
#             Android/gate-local fixture suites in parallel
#
# `cargo fmt --check`, the Rust toolchain pin check, and the documentation
# version check run on every invocation. Docs-only changes run nothing else.
# A path that matches no bucket, a change to this script, or a missing merge
# base (in branch mode) falls back to --full.
#
# Env:
#   GATE_LOCAL_BASE    override the branch comparison ref (implies --branch)
#   GATE_LOCAL_SERIAL  set to 1 to disable parallel execution (same as --serial)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

full=0
branch_mode=0
dry_run=0
serial="${GATE_LOCAL_SERIAL:-0}"
for arg in "$@"; do
  case "$arg" in
  --full) full=1 ;;
  --branch) branch_mode=1 ;;
  --dry-run) dry_run=1 ;;
  --serial) serial=1 ;;
  -h | --help)
    sed -n '3,42p' "$0" | sed 's/^# \{0,1\}//'
    exit 0
    ;;
  *)
    echo "gate-local: unknown argument: $arg (expected --branch, --full, --dry-run, or --serial)" >&2
    exit 2
    ;;
  esac
done

run_rust=0
rust_all=0
rust_paths=()
# Tauri shell crates (ice-box, ice-box-mobile) with changed sources.
shells=()

run_fe_typecheck=0
run_fe_vitest=0
run_fe_arch_vitest=0
run_fe_website_test=0
run_fe_mobile_demo=0

run_fixtures=0
# Changed shell scripts, syntax-checked in the fixtures bucket.
changed_scripts=()

add_unique() {
  local array_name="$1"
  local value="$2"
  local existing
  # Eval keeps this portable to bash 3.2 (macOS /bin/bash) without namerefs.
  eval "for existing in \${${array_name}[@]+\"\${${array_name}[@]}\"}; do
    if [[ \"\$existing\" == \"\$value\" ]]; then
      return 0
    fi
  done
  ${array_name}+=(\"\$value\")"
}

is_desktop_test_path() {
  case "$1" in
  apps/desktop/src/*.test.ts | apps/desktop/src/*.test.tsx | \
    apps/desktop/src/*/*.test.ts | apps/desktop/src/*/*.test.tsx | \
    apps/desktop/src/*/*/*.test.ts | apps/desktop/src/*/*/*.test.tsx | \
    apps/desktop/src/test/*)
    return 0
    ;;
  *)
    return 1
    ;;
  esac
}

# Classify one repository-relative path into fine-grained buckets. Returns 1
# for a path that matches no known area so the caller can fall back to --full.
classify() {
  local path="$1"

  # Cross-bucket inputs: files one bucket owns that another bucket's tests read.
  case "$path" in
  apps/desktop/src-tauri/src/application/* | crates/ice-app/Cargo.toml | \
    apps/website/vite.config.ts | apps/website/src/browser-shell.ts | \
    apps/website/mobile-demo.html | apps/website/src/main.js | \
    apps/mobile/src/shell.ts)
    run_fe_arch_vitest=1
    ;;
  esac
  case "$path" in
  # Release preflight fixtures read the real repo version sources and changelog.
  CHANGELOG.md | Cargo.toml | apps/desktop/package.json | apps/desktop/package-lock.json | \
    apps/mobile/package.json | apps/desktop/src-tauri/tauri.conf.json | \
    apps/mobile/src-tauri/tauri.conf.json)
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
    add_unique shells ice-box
    add_unique rust_paths "$path"
    ;;
  apps/mobile/src-tauri/* | apps/mobile/plugins/*)
    run_rust=1
    add_unique shells ice-box-mobile
    add_unique rust_paths "$path"
    ;;
  crates/*)
    run_rust=1
    add_unique rust_paths "$path"
    ;;
  configs/* | third_party/* | spikes/* | Cargo.toml | Cargo.lock | \
    rust-toolchain.toml | deny.toml | .cargo/*)
    run_rust=1
    rust_all=1
    ;;
  apps/desktop/*)
    run_fe_typecheck=1
    run_fe_vitest=1
    if ! is_desktop_test_path "$path"; then
      case "$path" in
      apps/desktop/src/* | apps/desktop/package.json | apps/desktop/package-lock.json | \
        apps/desktop/index.html | apps/desktop/public/* | apps/desktop/vite.config.ts)
        run_fe_website_test=1
        run_fe_mobile_demo=1
        ;;
      esac
    fi
    ;;
  apps/website/*)
    run_fe_typecheck=1
    run_fe_website_test=1
    run_fe_mobile_demo=1
    ;;
  apps/mobile/*)
    run_fe_typecheck=1
    run_fe_mobile_demo=1
    ;;
  package.json | package-lock.json)
    run_fe_typecheck=1
    run_fe_vitest=1
    run_fe_website_test=1
    run_fe_mobile_demo=1
    ;;
  scripts/*)
    run_fixtures=1
    if [[ "$path" == *.sh && -f "$path" ]]; then
      add_unique changed_scripts "$path"
    fi
    ;;
  *)
    return 1
    ;;
  esac
}

# The Tauri shells link GTK/WebKit on Linux; macOS and Windows build them with
# the system web view.
shell_deps_available() {
  [[ "$(uname -s)" != Linux ]] && return 0
  command -v pkg-config >/dev/null 2>&1 &&
    pkg-config --exists webkit2gtk-4.1 javascriptcoregtk-4.1 libsoup-3.0
}

# Resolve changed Rust paths to affected non-shell workspace crates (including
# transitive workspace reverse dependencies) using `cargo metadata`.
# Populates `rust_crates` (empty when `rust_all=1` or only shells changed) and
# `rust_has_lib` / `rust_has_bins` so binary-only crates do not pass `--lib`.
rust_crates=()
rust_has_lib=0
rust_has_bins=0
resolve_rust_crates() {
  if [[ "$run_rust" == 0 || "$rust_all" == 1 ]]; then
    return 0
  fi
  if [[ "${#rust_paths[@]}" -eq 0 ]]; then
    rust_all=1
    return 0
  fi
  if ! command -v python3 >/dev/null 2>&1; then
    rust_all=1
    return 0
  fi

  local resolved
  if ! resolved="$(
    python3 - "${rust_paths[@]}" <<'PY'
import json
import subprocess
import sys
from pathlib import Path

paths = [arg.strip() for arg in sys.argv[1:] if arg.strip()]
shells = {"ice-box", "ice-box-mobile"}
lib_kinds = {"lib", "rlib", "dylib", "cdylib", "staticlib", "proc-macro"}

proc = subprocess.run(
    ["cargo", "metadata", "--no-deps", "--format-version", "1"],
    capture_output=True,
    text=True,
)
if proc.returncode != 0:
    print("ALL")
    sys.exit(0)

meta = json.loads(proc.stdout)
root = Path(meta["workspace_root"]).resolve()
ws_ids = set(meta.get("workspace_members", []))
pkgs = [p for p in meta.get("packages", []) if p["id"] in ws_ids]
pkg_by_name = {p["name"]: p for p in pkgs}
pkg_names = set(pkg_by_name)

# Map relative manifest directory -> package name, longest prefix first.
dir_to_pkg = []
for p in pkgs:
    rel = Path(p["manifest_path"]).resolve().parent.relative_to(root).as_posix()
    dir_to_pkg.append((rel, p["name"]))
dir_to_pkg.sort(key=lambda item: len(item[0]), reverse=True)

# Reverse dependency graph across workspace members (normal + dev + build deps).
rev_deps = {name: set() for name in pkg_names}
for p in pkgs:
    for dep in p.get("dependencies", []):
        dep_name = dep["name"]
        if dep_name in rev_deps:
            rev_deps[dep_name].add(p["name"])

seed_non_shell = set()
for raw in paths:
    norm = raw.strip("/")
    matched = None
    for rel, name in dir_to_pkg:
        if norm == rel or norm.startswith(rel + "/"):
            matched = name
            break
    if matched is None:
        print("ALL")
        sys.exit(0)
    if matched not in shells:
        seed_non_shell.add(matched)

# Transitive reverse closure restricted to non-shell crates. Tauri shells are
# gated explicitly when their own directories change (or under --full).
affected = set(seed_non_shell)
queue = list(seed_non_shell)
while queue:
    cur = queue.pop()
    for parent in rev_deps.get(cur, ()):
        if parent in shells or parent in affected:
            continue
        affected.add(parent)
        queue.append(parent)

for name in sorted(affected):
    targets = pkg_by_name[name].get("targets", [])
    has_lib = 1 if any(
        kind in lib_kinds for t in targets for kind in t.get("kind", [])
    ) else 0
    has_bin = 1 if any("bin" in t.get("kind", []) for t in targets) else 0
    print(f"{name} {has_lib} {has_bin}")
PY
  )"; then
    rust_all=1
    return 0
  fi

  local crate has_lib has_bin
  while IFS=' ' read -r crate has_lib has_bin; do
    [[ -n "$crate" ]] || continue
    if [[ "$crate" == "ALL" ]]; then
      rust_all=1
      rust_crates=()
      rust_has_lib=0
      rust_has_bins=0
      return 0
    fi
    add_unique rust_crates "$crate"
    if [[ "${has_lib:-0}" == 1 ]]; then
      rust_has_lib=1
    fi
    if [[ "${has_bin:-0}" == 1 ]]; then
      rust_has_bins=1
    fi
  done <<<"$resolved"
}

diff_label="full"
changed=""
if [[ "$full" == 0 ]]; then
  if [[ -n "${GATE_LOCAL_BASE:-}" ]]; then
    branch_mode=1
  fi

  uncommitted=""
  if [[ "$branch_mode" == 0 ]] && git rev-parse --verify --quiet HEAD >/dev/null 2>&1; then
    uncommitted="$(
      {
        git diff --name-only --no-renames HEAD
        git ls-files --others --exclude-standard
      } | sort -u
    )"
  fi

  if [[ "$branch_mode" == 0 && -n "$uncommitted" ]]; then
    changed="$uncommitted"
    diff_label="working tree vs HEAD (pass --branch for full branch diff)"
  else
    base=""
    for ref in "${GATE_LOCAL_BASE:-}" origin/main main; do
      [[ -n "$ref" ]] || continue
      if base="$(git merge-base HEAD "$ref" 2>/dev/null)"; then
        diff_label="branch vs $ref"
        break
      fi
      base=""
    done
    if [[ -z "$base" ]]; then
      echo "gate-local: no merge base with main; running every check"
      full=1
      diff_label="full (no merge base)"
    else
      changed="$(
        {
          # --no-renames lists both sides of a rename so moving a file out of a
          # code area still selects the checks for that area.
          git diff --name-only --no-renames "$base"
          git ls-files --others --exclude-standard
        } | sort -u
      )"
    fi
  fi
fi

if [[ "$full" == 0 ]]; then
  while IFS= read -r path; do
    [[ -n "$path" ]] || continue
    if ! classify "$path"; then
      echo "gate-local: $path is outside the scoped buckets; running every check"
      full=1
      diff_label="full ($path)"
      break
    fi
  done <<<"$changed"
fi

if [[ "$full" == 1 ]]; then
  run_rust=1
  rust_all=1
  shells=(ice-box ice-box-mobile)
  run_fe_typecheck=1
  run_fe_vitest=1
  run_fe_arch_vitest=0
  run_fe_website_test=1
  run_fe_mobile_demo=1
  run_fixtures=1
fi

# Full Vitest already includes architecture.test.ts and appVersion.test.ts.
[[ "$run_fe_vitest" == 1 ]] && run_fe_arch_vitest=0

resolve_rust_crates

shells_runnable=0
if [[ "${#shells[@]}" -gt 0 ]] && shell_deps_available; then
  shells_runnable=1
fi

run_tun_integration=0
if [[ "$run_rust" == 1 ]]; then
  if [[ "$rust_all" == 1 ]]; then
    run_tun_integration=1
  else
    for crate in ${rust_crates[@]+"${rust_crates[@]}"}; do
      if [[ "$crate" == "ice-tun-sys" ]]; then
        run_tun_integration=1
        break
      fi
    done
  fi
fi

run_any_frontend=0
if [[ "$run_fe_typecheck" == 1 || "$run_fe_vitest" == 1 || "$run_fe_arch_vitest" == 1 || \
  "$run_fe_website_test" == 1 || "$run_fe_mobile_demo" == 1 ]]; then
  run_any_frontend=1
fi

scope=()
if [[ "$run_rust" == 1 ]]; then
  if [[ "$rust_all" == 1 ]]; then
    scope+=("rust(workspace)")
  elif [[ "${#rust_crates[@]}" -gt 0 ]]; then
    scope+=("rust(${rust_crates[*]})")
  fi
fi
if [[ "${#shells[@]}" -gt 0 ]]; then
  if [[ "$shells_runnable" == 1 ]]; then
    scope+=("shells(${shells[*]})")
  else
    scope+=("shells(${shells[*]}: skipped, GTK/WebKit missing)")
  fi
fi
if [[ "$run_any_frontend" == 1 ]]; then
  fe_parts=()
  [[ "$run_fe_typecheck" == 1 ]] && fe_parts+=(typecheck)
  [[ "$run_fe_vitest" == 1 ]] && fe_parts+=(vitest)
  [[ "$run_fe_arch_vitest" == 1 ]] && fe_parts+=(arch-vitest)
  [[ "$run_fe_website_test" == 1 ]] && fe_parts+=(website-test)
  [[ "$run_fe_mobile_demo" == 1 ]] && fe_parts+=(mobile-demo)
  scope+=("frontend(${fe_parts[*]})")
fi
[[ "$run_fixtures" == 1 ]] && scope+=(fixtures)

scope_label="${scope[*]:-fmt, toolchain pins, doc versions only}"
[[ "$full" == 1 ]] && scope_label+=" (full)"
echo "gate-local: diff = $diff_label"
echo "gate-local: scope = $scope_label"
[[ "$dry_run" == 1 ]] && exit 0

LOG_DIR="$(mktemp -d "${TMPDIR:-/tmp}/ice-box-gate-local-XXXXXX")"
ACTIVE_PIDS=()

kill_tree() {
  local pid="$1"
  if command -v pkill >/dev/null 2>&1; then
    pkill -TERM -P "$pid" 2>/dev/null || true
  fi
  kill "$pid" 2>/dev/null || true
}

cleanup() {
  local pid
  for pid in ${ACTIVE_PIDS[@]+"${ACTIVE_PIDS[@]}"}; do
    kill_tree "$pid"
  done
  rm -rf "$LOG_DIR"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

# Parallel task runner (bash 3.2 compatible).
# Usage:
#   batch_reset
#   batch_add "label" func_or_cmd [args...]
#   batch_wait   # returns non-zero if any task in the batch failed
BATCH_PIDS=()
BATCH_LABELS=()
BATCH_LOGS=()
BATCH_RCS=()
BATCH_STARTS=()

batch_reset() {
  BATCH_PIDS=()
  BATCH_LABELS=()
  BATCH_LOGS=()
  BATCH_RCS=()
  BATCH_STARTS=()
}

finish_task() {
  local rc=$?
  local pid
  for pid in ${ACTIVE_PIDS[@]+"${ACTIVE_PIDS[@]}"}; do
    kill_tree "$pid"
  done
  local elapsed=$((SECONDS - _TASK_T0))
  if [[ "$elapsed" -lt 0 ]]; then
    elapsed=0
  fi
  printf '%s %s\n' "$rc" "$elapsed" >"$_TASK_RC_FILE" 2>/dev/null || true
}

batch_add() {
  local label="$1"
  shift
  local start_s="$SECONDS"

  if [[ "$serial" == 1 ]]; then
    echo "== $label =="
    # Run the task in a subshell with `set -euo pipefail` so multi-step
    # functions abort on their first failing command even though the caller
    # temporarily clears `-e` to capture the exit status.
    set +e
    (
      set -euo pipefail
      "$@"
    )
    local rc=$?
    set -e
    local elapsed=$((SECONDS - start_s))
    if [[ "$elapsed" -lt 0 ]]; then
      elapsed=0
    fi
    if [[ "$rc" -ne 0 ]]; then
      echo "gate-local: FAILED $label (${elapsed}s, exit $rc)" >&2
      return "$rc"
    fi
    echo "gate-local: ok $label (${elapsed}s)"
    return 0
  fi

  local stem
  stem="$(mktemp "$LOG_DIR/task-XXXXXX")"
  local log_file="${stem}.log"
  local rc_file="${stem}.rc"

  (
    _TASK_T0="$SECONDS"
    _TASK_RC_FILE="$rc_file"
    ACTIVE_PIDS=()
    trap finish_task EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM
    set -euo pipefail
    "$@" >"$log_file" 2>&1
  ) &
  local pid=$!
  BATCH_PIDS+=("$pid")
  BATCH_LABELS+=("$label")
  BATCH_LOGS+=("$log_file")
  BATCH_RCS+=("$rc_file")
  BATCH_STARTS+=("$start_s")
  ACTIVE_PIDS+=("$pid")
}

batch_wait() {
  if [[ "$serial" == 1 || "${#BATCH_PIDS[@]}" -eq 0 ]]; then
    return 0
  fi
  local i=0
  local count="${#BATCH_PIDS[@]}"
  local failed=0
  while [[ "$i" -lt "$count" ]]; do
    wait "${BATCH_PIDS[$i]}" 2>/dev/null || true
    local rc=1
    local elapsed=$((SECONDS - BATCH_STARTS[$i]))
    if [[ -f "${BATCH_RCS[$i]}" ]]; then
      read -r rc elapsed <"${BATCH_RCS[$i]}" || true
    fi
    if [[ "$elapsed" -lt 0 ]]; then
      elapsed=0
    fi
    if [[ "$rc" -eq 0 ]]; then
      echo "gate-local: ok ${BATCH_LABELS[$i]} (${elapsed}s)"
    else
      echo "gate-local: FAILED ${BATCH_LABELS[$i]} (${elapsed}s, exit $rc)" >&2
      echo "--- output: ${BATCH_LABELS[$i]} ---" >&2
      cat "${BATCH_LOGS[$i]}" >&2 || true
      echo "--- end: ${BATCH_LABELS[$i]} ---" >&2
      failed=1
    fi
    i=$((i + 1))
  done
  ACTIVE_PIDS=()
  return "$failed"
}

run_in_dir() {
  local dir="$1"
  shift
  (cd "$dir" && "$@")
}

run_rust_bucket() {
  local has_non_shell=0
  if [[ "$rust_all" == 1 || "${#rust_crates[@]}" -gt 0 ]]; then
    has_non_shell=1
  fi
  local has_shells=0
  if [[ "${#shells[@]}" -gt 0 && "$shells_runnable" == 1 ]]; then
    has_shells=1
  fi

  if [[ "${#shells[@]}" -gt 0 && "$shells_runnable" == 0 ]]; then
    echo "gate-local: WARNING: ${shells[*]} changed but GTK/WebKit dev packages are missing;" >&2
    echo "gate-local: WARNING: these crates are not compiled here. CI scripts/gate.sh covers them." >&2
  fi

  if [[ "$has_non_shell" == 0 && "$has_shells" == 0 ]]; then
    return 0
  fi

  # 1. Single combined clippy invocation so shared workspace dependencies are
  # checked once rather than once for crates and again for the Tauri shell.
  if [[ "$rust_all" == 1 ]]; then
    if [[ "$has_shells" == 1 && "${#shells[@]}" -eq 2 ]]; then
      echo "== cargo clippy (workspace) =="
      cargo clippy --workspace --all-targets -- -D warnings || return $?
    else
      local exclude_args=()
      local s
      for s in ice-box ice-box-mobile; do
        local keep=0
        if [[ "$has_shells" == 1 ]]; then
          local chosen
          for chosen in "${shells[@]}"; do
            if [[ "$chosen" == "$s" ]]; then
              keep=1
            fi
          done
        fi
        if [[ "$keep" == 0 ]]; then
          exclude_args+=(--exclude "$s")
        fi
      done
      echo "== cargo clippy (workspace) =="
      cargo clippy --workspace --all-targets ${exclude_args[@]+"${exclude_args[@]}"} -- -D warnings || return $?
    fi
  else
    local clippy_pkgs=()
    local c
    for c in ${rust_crates[@]+"${rust_crates[@]}"}; do
      clippy_pkgs+=(-p "$c")
    done
    if [[ "$has_shells" == 1 ]]; then
      for c in "${shells[@]}"; do
        clippy_pkgs+=(-p "$c")
      done
    fi
    echo "== cargo clippy (${clippy_pkgs[*]}) =="
    cargo clippy "${clippy_pkgs[@]}" --all-targets -- -D warnings || return $?
  fi

  # 2. Non-shell Rust unit tests (lib + bins, e.g. ice-helper install tests).
  if [[ "$rust_all" == 1 ]]; then
    echo "== cargo test (non-shell lib + bins) =="
    cargo test --workspace --lib --bins --exclude ice-box --exclude ice-box-mobile || return $?
  elif [[ "${#rust_crates[@]}" -gt 0 ]]; then
    local pkg_args=()
    local c
    for c in "${rust_crates[@]}"; do
      pkg_args+=(-p "$c")
    done
    local target_args=()
    if [[ "$rust_has_lib" == 1 ]]; then
      target_args+=(--lib)
    fi
    if [[ "$rust_has_bins" == 1 ]]; then
      target_args+=(--bins)
    fi
    if [[ "${#target_args[@]}" -gt 0 ]]; then
      echo "== cargo test (${rust_crates[*]} ${target_args[*]}) =="
      cargo test "${pkg_args[@]}" "${target_args[@]}" || return $?
    fi
  fi

  # 3. Host-free TUN integration tests when ice-tun-sys is affected.
  if [[ "$run_tun_integration" == 1 ]]; then
    echo "== cargo test (ice-tun-sys integration) =="
    cargo test -p ice-tun-sys --tests || return $?
  fi

  # 4. Tauri shell library tests when runnable.
  if [[ "$has_shells" == 1 ]]; then
    local shell_args=()
    local c
    for c in "${shells[@]}"; do
      shell_args+=(-p "$c")
    done
    echo "== cargo test (Tauri shells lib: ${shells[*]}) =="
    cargo test "${shell_args[@]}" --lib || return $?
  fi
}

run_mobile_demo_smoke() {
  (
    set -euo pipefail
    cd "$ROOT/apps/website"
    npm run build
    # Reuse cached Playwright Chromium so local runs avoid a network manifest
    # check on every commit; install automatically on first run.
    if ! node -e 'const fs = require("fs"); const { chromium } = require("playwright"); process.exit(fs.existsSync(chromium.executablePath()) ? 0 : 1);' 2>/dev/null; then
      npx playwright install chromium
    fi
    npm run test:mobile-demo
  )
}

run_frontend_bucket() {
  if [[ "$run_fe_typecheck" == 1 ]]; then
    batch_reset
    batch_add "tsc (desktop)" run_in_dir "$ROOT/apps/desktop" npm run typecheck
    batch_add "tsc (website)" run_in_dir "$ROOT/apps/website" npm run typecheck
    batch_add "tsc (mobile)" run_in_dir "$ROOT/apps/mobile" npm run typecheck
    batch_wait || return $?
  fi

  batch_reset
  if [[ "$run_fe_vitest" == 1 ]]; then
    batch_add "vitest (desktop)" run_in_dir "$ROOT/apps/desktop" npm test
  elif [[ "$run_fe_arch_vitest" == 1 ]]; then
    batch_add "vitest (architecture + version)" run_in_dir "$ROOT/apps/desktop" npx vitest run src/api/architecture.test.ts src/lib/appVersion.test.ts
  fi
  if [[ "$run_fe_website_test" == 1 ]]; then
    batch_add "website installer links" run_in_dir "$ROOT/apps/website" npm test
  fi
  if [[ "$run_fe_mobile_demo" == 1 ]]; then
    batch_add "website build + mobile demo smoke" run_mobile_demo_smoke
  fi
  batch_wait || return $?
}

run_bash_syntax_checks() {
  local script
  for script in ${changed_scripts[@]+"${changed_scripts[@]}"}; do
    bash -n "$script" || return $?
  done
}

run_fixtures_bucket() {
  batch_reset
  if [[ "${#changed_scripts[@]}" -gt 0 ]]; then
    batch_add "bash -n (changed scripts)" run_bash_syntax_checks
  fi

  local fixture
  for fixture in \
    scripts/test-gate-local.sh \
    scripts/test-merge-updater-latest.sh \
    scripts/test-check-toolchain-pins.sh \
    scripts/test-check-doc-versions.sh \
    scripts/test-changelog-release.sh \
    scripts/test-verify-ci-status.sh \
    scripts/test-verify-release-environment.sh \
    scripts/test-release-preflight.sh \
    scripts/test-verify-updater-signatures.sh \
    scripts/test-verify-signing-key.sh \
    scripts/test-verify-release-assets.sh \
    scripts/test-gh-release-state.sh \
    scripts/test-verify-android-keystore.sh \
    scripts/test-android-version-code.sh \
    scripts/test-verify-android-apk.sh \
    scripts/test-check-glibc-floor.sh \
    scripts/test-verify-linux-bundles.sh \
    scripts/test-retry-tauri-build.sh; do
    if [[ -f "$ROOT/$fixture" ]]; then
      batch_add "$(basename "$fixture" .sh)" bash "$ROOT/$fixture"
    fi
  done
  batch_wait
}

gate_start="$SECONDS"

# Phase 0: fast repo-wide invariants (always run).
batch_reset
batch_add "cargo fmt --check" cargo fmt --check
batch_add "rust toolchain pins" bash "$ROOT/scripts/check-toolchain-pins.sh"
batch_add "documentation versions" bash "$ROOT/scripts/check-doc-versions.sh"
batch_wait

# Phase 1: independent buckets run in parallel.
batch_reset
if [[ "$run_rust" == 1 || "${#shells[@]}" -gt 0 ]]; then
  batch_add "rust bucket" run_rust_bucket
fi
if [[ "$run_any_frontend" == 1 ]]; then
  batch_add "frontend bucket" run_frontend_bucket
fi
if [[ "$run_fixtures" == 1 ]]; then
  batch_add "fixtures bucket" run_fixtures_bucket
fi
batch_wait

gate_elapsed=$((SECONDS - gate_start))
if [[ "$gate_elapsed" -lt 0 ]]; then
  gate_elapsed=0
fi
echo "gate-local: OK (${gate_elapsed}s)"
