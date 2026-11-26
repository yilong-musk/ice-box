#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Fixture tests for scripts/gate-local.sh scope selection (diff modes, crate
# reverse-dependency closure, fine-grained frontend sub-buckets, fallbacks).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/ice-box-gate-local-test-XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

fail() {
  echo "test-gate-local: $*" >&2
  exit 1
}

assert_contains() {
  local name="$1"
  local haystack="$2"
  local needle="$3"
  if ! printf '%s\n' "$haystack" | grep -Fq "$needle"; then
    fail "$name: expected '$needle' in output:\n$haystack"
  fi
}

assert_not_contains() {
  local name="$1"
  local haystack="$2"
  local needle="$3"
  if printf '%s\n' "$haystack" | grep -Fq "$needle"; then
    fail "$name: unexpected '$needle' in output:\n$haystack"
  fi
}

# Build a lightweight git repository clone whose tracked tree shares the real
# Cargo.toml / crate Cargo.toml manifests so `cargo metadata` resolves the exact
# workspace graph without copying target/ or node_modules/.
REPO="$TMP/repo"
mkdir -p "$REPO"
python3 - "$ROOT" "$REPO" <<'PY'
import os
import shutil
import sys
from pathlib import Path

src = Path(sys.argv[1])
dst = Path(sys.argv[2])

# Copy top-level Cargo files, scripts/gate-local.sh, and every workspace
# member's Cargo.toml + minimal stub lib/main so `cargo metadata --no-deps`
# succeeds inside the temporary git repository.
for rel in ("Cargo.toml", "Cargo.lock", "scripts/gate-local.sh"):
    target = dst / rel
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(src / rel, target)

for manifest in sorted(src.glob("crates/*/Cargo.toml")) + [
    src / "apps/desktop/src-tauri/Cargo.toml",
    src / "apps/mobile/src-tauri/Cargo.toml",
    src / "apps/mobile/plugins/tunnel/Cargo.toml",
]:
    rel = manifest.relative_to(src)
    target = dst / rel
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(manifest, target)
    src_dir = target.parent / "src"
    src_dir.mkdir(exist_ok=True)
    if (manifest.parent / "src/lib.rs").exists():
        (src_dir / "lib.rs").write_text("", encoding="utf-8")
    if (manifest.parent / "src/main.rs").exists():
        (src_dir / "main.rs").write_text("fn main() {}\n", encoding="utf-8")
    if (manifest.parent / "build.rs").exists():
        (target.parent / "build.rs").write_text("fn main() {}\n", encoding="utf-8")

# Seed placeholder files for non-Rust buckets.
for rel in (
    "README.md",
    "CHANGELOG.md",
    "apps/desktop/package.json",
    "apps/desktop/src/pages/Home.tsx",
    "apps/desktop/src/pages/Settings.test.tsx",
    "apps/desktop/src-tauri/src/autostart.rs",
    "apps/desktop/src-tauri/src/application/common.rs",
    "apps/website/src/downloads.ts",
    "apps/website/src/main.js",
    "apps/mobile/src/App.tsx",
    "scripts/dev.sh",
):
    target = dst / rel
    target.parent.mkdir(parents=True, exist_ok=True)
    if not target.exists():
        target.write_text("seed\n", encoding="utf-8")
PY

git -C "$REPO" init -q -b main
git -C "$REPO" config user.email "test@example.com"
git -C "$REPO" config user.name "Test"
git -C "$REPO" add -A
git -C "$REPO" commit -q -m "base"

reset_worktree() {
  git -C "$REPO" reset -q --hard HEAD
  git -C "$REPO" clean -qfd
}

run_dry() {
  (cd "$REPO" && bash scripts/gate-local.sh --dry-run "$@")
}

echo "== docs-only change =="
git -C "$REPO" checkout -q -b feat/test main
printf 'doc edit\n' >>"$REPO/README.md"
out="$(run_dry)"
assert_contains "docs-only" "$out" "fmt, toolchain pins, doc versions only"
reset_worktree

echo "== rust crate scoping: leaf crate (ice-elevate) =="
printf '// edit\n' >>"$REPO/crates/ice-elevate/src/lib.rs"
out="$(run_dry)"
assert_contains "leaf-crate" "$out" "rust(ice-elevate)"
assert_not_contains "leaf-crate" "$out" "ice-config"
assert_not_contains "leaf-crate" "$out" "frontend("
assert_not_contains "leaf-crate" "$out" "fixtures"
reset_worktree

echo "== rust crate scoping: reverse deps (ice-helper -> ice-tun-sys) =="
printf '// edit\n' >>"$REPO/crates/ice-helper/src/lib.rs"
out="$(run_dry)"
assert_contains "helper-revdeps" "$out" "rust(ice-helper ice-tun-sys)"
assert_not_contains "helper-revdeps" "$out" "ice-config"
assert_not_contains "helper-revdeps" "$out" "shells("
reset_worktree

echo "== rust shell-only change (apps/desktop/src-tauri/src/autostart.rs) =="
printf '// edit\n' >>"$REPO/apps/desktop/src-tauri/src/autostart.rs"
out="$(run_dry)"
assert_contains "shell-only" "$out" "shells(ice-box"
assert_not_contains "shell-only" "$out" "rust("
assert_not_contains "shell-only" "$out" "frontend("
reset_worktree

echo "== rust application boundary triggers fast arch-vitest, not full vitest =="
printf '// edit\n' >>"$REPO/apps/desktop/src-tauri/src/application/common.rs"
out="$(run_dry)"
assert_contains "app-boundary" "$out" "shells(ice-box"
assert_contains "app-boundary" "$out" "frontend(arch-vitest)"
assert_not_contains "app-boundary" "$out" "mobile-demo"
assert_not_contains "app-boundary" "$out" "frontend(typecheck"
reset_worktree

echo "== desktop test-only skips website build + mobile demo =="
printf '// edit\n' >>"$REPO/apps/desktop/src/pages/Settings.test.tsx"
out="$(run_dry)"
assert_contains "desktop-test-only" "$out" "frontend(typecheck vitest)"
assert_not_contains "desktop-test-only" "$out" "mobile-demo"
assert_not_contains "desktop-test-only" "$out" "website-test"
reset_worktree

echo "== website non-boundary edit skips desktop vitest =="
printf '// edit\n' >>"$REPO/apps/website/src/downloads.ts"
out="$(run_dry)"
assert_contains "website-only" "$out" "frontend(typecheck website-test mobile-demo)"
assert_not_contains "website-only" "$out" "vitest"
reset_worktree

echo "== website main.js adds fast arch-vitest =="
printf '// edit\n' >>"$REPO/apps/website/src/main.js"
out="$(run_dry)"
assert_contains "website-main" "$out" "frontend(typecheck arch-vitest website-test mobile-demo)"
reset_worktree

echo "== working tree vs HEAD ignores earlier branch commits; --branch includes them =="
printf '// frontend commit\n' >>"$REPO/apps/website/src/downloads.ts"
printf '# script commit\n' >>"$REPO/scripts/dev.sh"
git -C "$REPO" add -A
git -C "$REPO" commit -q -m "earlier branch work"

# Clean tree on feature branch defaults to branch diff vs main.
out_clean="$(run_dry)"
assert_contains "clean-branch" "$out_clean" "diff = branch vs main"
assert_contains "clean-branch" "$out_clean" "frontend(typecheck website-test mobile-demo)"
assert_contains "clean-branch" "$out_clean" "fixtures"

# Uncommitted Rust edit defaults to working tree vs HEAD (skips website & fixtures).
printf '// follow-up rust edit\n' >>"$REPO/crates/ice-elevate/src/lib.rs"
out_wt="$(run_dry)"
assert_contains "wt-vs-head" "$out_wt" "diff = working tree vs HEAD"
assert_contains "wt-vs-head" "$out_wt" "rust(ice-elevate)"
assert_not_contains "wt-vs-head" "$out_wt" "frontend("
assert_not_contains "wt-vs-head" "$out_wt" "fixtures"

# Explicit --branch includes both earlier branch commits and working tree edit.
out_branch="$(run_dry --branch)"
assert_contains "explicit-branch" "$out_branch" "diff = branch vs main"
assert_contains "explicit-branch" "$out_branch" "rust(ice-elevate)"
assert_contains "explicit-branch" "$out_branch" "frontend(typecheck website-test mobile-demo)"
assert_contains "explicit-branch" "$out_branch" "fixtures"
reset_worktree

echo "== gate-local.sh self-edit falls back to full =="
printf '# edit\n' >>"$REPO/scripts/gate-local.sh"
out="$(run_dry)"
assert_contains "self-edit" "$out" "(full)"
assert_contains "self-edit" "$out" "rust(workspace)"
assert_contains "self-edit" "$out" "fixtures"
reset_worktree

echo "test-gate-local: OK"
