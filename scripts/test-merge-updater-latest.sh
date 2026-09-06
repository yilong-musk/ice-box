#!/usr/bin/env bash
# Fixture tests for scripts/merge-updater-latest.sh.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SCRIPT="$ROOT/scripts/merge-updater-latest.sh"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/ice-box-updater-XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

fail() {
  echo "test-merge-updater-latest: $*" >&2
  exit 1
}

write_happy() {
  local dir="$1"
  mkdir -p "$dir"
  printf 'darwin-archive\n' >"$dir/ice-box.app.tar.gz"
  printf 'DARWIN-MINISIGN\nline-two\n' >"$dir/ice-box.app.tar.gz.sig"
  printf 'windows-installer\n' >"$dir/ice-box_0.1.5_x64-setup.exe"
  printf 'WINDOWS-MINISIGN\nline-two\n' >"$dir/ice-box_0.1.5_x64-setup.exe.sig"
  printf 'decoy\n' >"$dir/ice-box_0.1.5_aarch64.dmg"
  printf 'Release notes for 0.1.5\n\n- auto update\n' >"$dir/notes.md"
}

assert_json() {
  local json="$1"
  ICE_BOX_UPDATER_JSON="$json" python3 - <<'PY'
import json, os, sys

data = json.loads(os.environ["ICE_BOX_UPDATER_JSON"])
errors = []

def expect(cond, message):
    if not cond:
        errors.append(message)

expect(data.get("version") == "0.1.5", f"version: {data.get('version')!r}")
expect(data.get("pub_date") == "2026-09-06T12:00:00Z", f"pub_date: {data.get('pub_date')!r}")
expect("auto update" in data.get("notes", ""), f"notes: {data.get('notes')!r}")
platforms = data.get("platforms") or {}
darwin = platforms.get("darwin-aarch64") or {}
windows = platforms.get("windows-x86_64") or {}
expect(set(platforms) == {"darwin-aarch64", "windows-x86_64"}, f"platforms: {sorted(platforms)}")
expect(
    darwin.get("url")
    == "https://github.com/yilong-musk/ice-box/releases/download/v0.1.5/ice-box.app.tar.gz",
    f"darwin url: {darwin.get('url')!r}",
)
expect(
    windows.get("url")
    == "https://github.com/yilong-musk/ice-box/releases/download/v0.1.5/ice-box_0.1.5_x64-setup.exe",
    f"windows url: {windows.get('url')!r}",
)
expect(
    darwin.get("signature") == "DARWIN-MINISIGN\nline-two\n",
    f"darwin signature is not the full .sig text: {darwin.get('signature')!r}",
)
expect(
    windows.get("signature") == "WINDOWS-MINISIGN\nline-two\n",
    f"windows signature is not the full .sig text: {windows.get('signature')!r}",
)
expect("dmg" not in json.dumps(data), "dmg decoy leaked into latest.json")
if errors:
    print("\n".join(errors), file=sys.stderr)
    sys.exit(1)
PY
}

echo "== happy path =="
write_happy "$TMP/happy"
OUT="$(
  GITHUB_REPOSITORY="yilong-musk/ice-box" \
  ICE_BOX_UPDATER_PUB_DATE="2026-09-06T12:00:00Z" \
    bash "$SCRIPT" "$TMP/happy" v0.1.5 "$TMP/happy/notes.md"
)"
assert_json "$OUT"

echo "== nested bundle dirs (CI artifact layout) =="
mkdir -p "$TMP/nested/macos" "$TMP/nested/dmg"
write_happy "$TMP/nested-flat"
mv "$TMP/nested-flat/ice-box.app.tar.gz" "$TMP/nested/macos/"
mv "$TMP/nested-flat/ice-box.app.tar.gz.sig" "$TMP/nested/macos/"
mv "$TMP/nested-flat/ice-box_0.1.5_x64-setup.exe" "$TMP/nested/"
mv "$TMP/nested-flat/ice-box_0.1.5_x64-setup.exe.sig" "$TMP/nested/"
mv "$TMP/nested-flat/ice-box_0.1.5_aarch64.dmg" "$TMP/nested/dmg/"
mv "$TMP/nested-flat/notes.md" "$TMP/nested/notes.md"
NESTED_OUT="$(
  GITHUB_REPOSITORY="yilong-musk/ice-box" \
  ICE_BOX_UPDATER_PUB_DATE="2026-09-06T12:00:00Z" \
    bash "$SCRIPT" "$TMP/nested" v0.1.5 "$TMP/nested/notes.md"
)"
assert_json "$NESTED_OUT"

echo "== missing darwin archive =="
write_happy "$TMP/no-darwin"
rm -f "$TMP/no-darwin/ice-box.app.tar.gz" "$TMP/no-darwin/ice-box.app.tar.gz.sig"
if bash "$SCRIPT" "$TMP/no-darwin" v0.1.5 "$TMP/no-darwin/notes.md" >"$TMP/out.json" 2>"$TMP/err.txt"; then
  fail "expected failure when darwin archive is missing"
fi
grep -q "darwin" "$TMP/err.txt" || fail "missing-darwin error should mention darwin"

echo "== missing windows signature =="
write_happy "$TMP/no-win-sig"
rm -f "$TMP/no-win-sig/ice-box_0.1.5_x64-setup.exe.sig"
if bash "$SCRIPT" "$TMP/no-win-sig" v0.1.5 "$TMP/no-win-sig/notes.md" >"$TMP/out.json" 2>"$TMP/err.txt"; then
  fail "expected failure when windows .sig is missing"
fi
grep -qi "signature" "$TMP/err.txt" || fail "missing-sig error should mention signature"

echo "== empty signature =="
write_happy "$TMP/empty-sig"
printf '   \n' >"$TMP/empty-sig/ice-box.app.tar.gz.sig"
if bash "$SCRIPT" "$TMP/empty-sig" v0.1.5 "$TMP/empty-sig/notes.md" >"$TMP/out.json" 2>"$TMP/err.txt"; then
  fail "expected failure when a .sig file is empty"
fi
grep -qi "empty" "$TMP/err.txt" || fail "empty-sig error should mention empty"

echo "== missing notes file =="
write_happy "$TMP/no-notes"
if bash "$SCRIPT" "$TMP/no-notes" v0.1.5 "$TMP/no-notes/missing.md" >"$TMP/out.json" 2>"$TMP/err.txt"; then
  fail "expected failure when notes file is missing"
fi
grep -qi "notes" "$TMP/err.txt" || fail "missing-notes error should mention notes"

echo "test-merge-updater-latest: OK"
