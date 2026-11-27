#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Fixture tests for scripts/gate-local.sh scope selection (diff modes, crate
# reverse-dependency closure, fine-grained frontend sub-buckets, fallbacks).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/ice-box-gate-local-test-XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

fail() {
  printf 'test-gate-local: %b\n' "$*" >&2
  exit 1
}

assert_contains() {
  local name="$1"
  local haystack="$2"
  local needle="$3"
  if ! printf '%s\n' "$haystack" | grep -Fq -- "$needle"; then
    fail "$name: expected '$needle' in output:\n$haystack"
  fi
}

assert_not_contains() {
  local name="$1"
  local haystack="$2"
  local needle="$3"
  if printf '%s\n' "$haystack" | grep -Fq -- "$needle"; then
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

# Seed minimal Phase 0 scripts so execution tests can run without the full repo.
for rel in ("scripts/check-toolchain-pins.sh", "scripts/check-doc-versions.sh"):
    target = dst / rel
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text("#!/usr/bin/env bash\nexit 0\n", encoding="utf-8")

# Seed placeholder files for non-Rust buckets.
for rel in (
    "README.md",
    "CHANGELOG.md",
    "apps/desktop/package.json",
    "apps/desktop/src/pages/Home.tsx",
    "apps/desktop/src/pages/Settings.test.tsx",
    "apps/desktop/src-tauri/src/app_update.rs",
    "crates/ice-runtime/src/application/common.rs",
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

echo "== rust crate scoping: leaf crate (ice-elevate -> ice-runtime -> ice-cli) =="
printf '// edit\n' >>"$REPO/crates/ice-elevate/src/lib.rs"
out="$(run_dry)"
assert_contains "leaf-crate" "$out" "rust(ice-cli ice-elevate ice-runtime)"
assert_not_contains "leaf-crate" "$out" "ice-config"
assert_not_contains "leaf-crate" "$out" "frontend("
assert_not_contains "leaf-crate" "$out" "fixtures"
reset_worktree

echo "== rust crate scoping: reverse deps (ice-helper -> ice-tun-sys -> ice-runtime -> ice-cli) =="
printf '// edit\n' >>"$REPO/crates/ice-helper/src/lib.rs"
out="$(run_dry)"
assert_contains "helper-revdeps" "$out" "rust(ice-cli ice-helper ice-runtime ice-tun-sys)"
assert_not_contains "helper-revdeps" "$out" "ice-config"
assert_not_contains "helper-revdeps" "$out" "shells("
reset_worktree

echo "== rust shell-only change (apps/desktop/src-tauri/src/app_update.rs) =="
printf '// edit\n' >>"$REPO/apps/desktop/src-tauri/src/app_update.rs"
out="$(run_dry)"
assert_contains "shell-only" "$out" "shells(ice-box"
assert_not_contains "shell-only" "$out" "rust("
assert_not_contains "shell-only" "$out" "frontend("
reset_worktree

echo "== rust application boundary triggers fast arch-vitest, not full vitest =="
printf '// edit\n' >>"$REPO/crates/ice-runtime/src/application/common.rs"
out="$(run_dry)"
assert_contains "app-boundary" "$out" "rust(ice-cli ice-runtime)"
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
assert_contains "wt-vs-head" "$out_wt" "rust(ice-cli ice-elevate ice-runtime)"
assert_not_contains "wt-vs-head" "$out_wt" "frontend("
assert_not_contains "wt-vs-head" "$out_wt" "fixtures"

# Explicit --branch includes both earlier branch commits and working tree edit.
out_branch="$(run_dry --branch)"
assert_contains "explicit-branch" "$out_branch" "diff = branch vs main"
assert_contains "explicit-branch" "$out_branch" "rust(ice-cli ice-elevate ice-runtime)"
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

echo "== help output includes GATE_LOCAL_SERIAL =="
help_out="$(cd "$REPO" && bash scripts/gate-local.sh --help)"
assert_contains "help-env" "$help_out" "GATE_LOCAL_SERIAL"

echo "== additional scope selections (bin-only crate, plugin, desktop src, CHANGELOG) =="
printf '// edit\n' >>"$REPO/crates/ice-tun-launcher/src/main.rs"
out="$(run_dry)"
assert_contains "bin-only-scope" "$out" "rust(ice-tun-launcher)"
assert_not_contains "bin-only-scope" "$out" "ice-config"
reset_worktree

printf '// edit\n' >>"$REPO/apps/mobile/plugins/tunnel/src/lib.rs"
out="$(run_dry)"
assert_contains "mobile-plugin" "$out" "rust(tauri-plugin-tunnel)"
assert_contains "mobile-plugin" "$out" "shells(ice-box-mobile"
reset_worktree

printf '// edit\n' >>"$REPO/apps/desktop/src/pages/Home.tsx"
out="$(run_dry)"
assert_contains "desktop-src" "$out" "frontend(typecheck vitest website-test mobile-demo)"
reset_worktree

printf 'note\n' >>"$REPO/CHANGELOG.md"
out="$(run_dry)"
assert_contains "changelog-fixtures" "$out" "fixtures"
assert_not_contains "changelog-fixtures" "$out" "rust("
assert_not_contains "changelog-fixtures" "$out" "frontend("
reset_worktree

# Build shims for execution tests: delegates `cargo metadata` to the real cargo
# binary while recording `cargo clippy` / `cargo test` / `npm` invocations and
# allowing controlled failure injection.
REAL_CARGO="$(command -v cargo)"
FAKEBIN="$TMP/fakebin"
mkdir -p "$FAKEBIN"
cat >"$FAKEBIN/cargo" <<EOF
#!/usr/bin/env bash
if [[ "\${1:-}" == "metadata" ]]; then
  exec "$REAL_CARGO" "\$@"
fi
if [[ -n "\${CARGO_LOG:-}" ]]; then
  printf '%s\n' "\$*" >>"\$CARGO_LOG"
fi
case "\${1:-}" in
fmt) exit 0 ;;
clippy) exit "\${FAKE_CLIPPY_RC:-0}" ;;
test) exit "\${FAKE_TEST_RC:-0}" ;;
*) exit 0 ;;
esac
EOF
chmod +x "$FAKEBIN/cargo"

cat >"$FAKEBIN/npm" <<'EOF'
#!/usr/bin/env bash
if [[ -n "${NPM_LOG:-}" ]]; then
  printf '%s\n' "$*" >>"$NPM_LOG"
fi
if [[ "${1:-}" == "run" && "${2:-}" == "typecheck" ]]; then
  exit "${FAKE_TSC_RC:-0}"
fi
if [[ "${1:-}" == "test" ]]; then
  exit "${FAKE_VITEST_RC:-0}"
fi
exit 0
EOF
chmod +x "$FAKEBIN/npm"

run_exec() {
  (cd "$REPO" && PATH="$FAKEBIN:$PATH" bash scripts/gate-local.sh "$@")
}

echo "== execution: binary-only crate omits --lib from cargo test =="
CARGO_LOG="$TMP/bin-only.log"
printf '// edit\n' >>"$REPO/crates/ice-tun-launcher/src/main.rs"
CARGO_LOG="$CARGO_LOG" run_exec >/dev/null
cargo_calls="$(cat "$CARGO_LOG")"
assert_contains "bin-only-exec" "$cargo_calls" "test -p ice-tun-launcher --bins"
assert_not_contains "bin-only-exec" "$cargo_calls" "--lib"
reset_worktree

echo "== execution: multi-crate + single-shell workspace exclusion under set -e =="
CARGO_LOG="$TMP/multi-crate.log"
printf '// edit\n' >>"$REPO/crates/ice-elevate/src/lib.rs"
printf '// edit\n' >>"$REPO/crates/ice-spawn/src/lib.rs"
CARGO_LOG="$CARGO_LOG" run_exec >/dev/null
cargo_calls="$(cat "$CARGO_LOG")"
assert_contains "multi-crate-exec" "$cargo_calls" "clippy -p ice-cli -p ice-core -p ice-elevate"
reset_worktree

CARGO_LOG="$TMP/ws-one-shell.log"
printf '\n' >>"$REPO/Cargo.toml"
printf '// edit\n' >>"$REPO/apps/desktop/src-tauri/src/app_update.rs"
CARGO_LOG="$CARGO_LOG" run_exec >/dev/null
cargo_calls="$(cat "$CARGO_LOG")"
assert_contains "ws-one-shell-exec" "$cargo_calls" "clippy --workspace --all-targets"
reset_worktree

echo "== execution: clippy failure is not swallowed by passing cargo test =="
for mode in parallel serial; do
  CARGO_LOG="$TMP/clippy-$mode.log"
  : >"$CARGO_LOG"
  printf '// edit\n' >>"$REPO/crates/ice-elevate/src/lib.rs"
  args=()
  [[ "$mode" == "serial" ]] && args=(--serial)
  if CARGO_LOG="$CARGO_LOG" FAKE_CLIPPY_RC=101 FAKE_TEST_RC=0 run_exec ${args[@]+"${args[@]}"} >/dev/null 2>&1; then
    fail "clippy-fail-$mode: expected non-zero exit when clippy fails"
  fi
  cargo_calls="$(cat "$CARGO_LOG")"
  assert_not_contains "clippy-fail-$mode" "$cargo_calls" "test "
  reset_worktree
done

echo "== execution: tsc failure is not swallowed by passing vitest =="
for mode in parallel serial; do
  NPM_LOG="$TMP/tsc-$mode.log"
  : >"$NPM_LOG"
  printf '// edit\n' >>"$REPO/apps/desktop/src/pages/Settings.test.tsx"
  args=()
  [[ "$mode" == "serial" ]] && args=(--serial)
  if NPM_LOG="$NPM_LOG" FAKE_TSC_RC=2 FAKE_VITEST_RC=0 run_exec ${args[@]+"${args[@]}"} >/dev/null 2>&1; then
    fail "tsc-fail-$mode: expected non-zero exit when typecheck fails"
  fi
  npm_calls="$(cat "$NPM_LOG")"
  assert_not_contains "tsc-fail-$mode" "$npm_calls" "test"
  reset_worktree
done

echo "== execution: bash -n failure on earlier script is not swallowed =="
for mode in parallel serial; do
  printf 'if then\n' >"$REPO/scripts/a-bad.sh"
  printf '#!/usr/bin/env bash\necho ok\n' >"$REPO/scripts/z-good.sh"
  args=()
  [[ "$mode" == "serial" ]] && args=(--serial)
  if run_exec ${args[@]+"${args[@]}"} >/dev/null 2>&1; then
    fail "bash-n-fail-$mode: expected non-zero exit when first script has syntax error"
  fi
  reset_worktree
done

echo "test-gate-local: OK"
