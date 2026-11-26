#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Fixture checks for scripts/verify-updater-signatures.sh.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SCRIPT="$ROOT/scripts/verify-updater-signatures.sh"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/ice-box-updater-signatures-XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

fail() {
  echo "test-verify-updater-signatures: $*" >&2
  exit 1
}

python3 - "$TMP" <<'PY'
import base64
import json
import sys
from pathlib import Path

root = Path(sys.argv[1])
display_id = bytes.fromhex("94954A1C99C12378")
stored_id = display_id[::-1]


def write_fixture(
    name: str,
    signature_id: bytes,
    *,
    mac: bool = True,
    wrapped: bool = True,
    version: str | None = "0.2.6",
) -> Path:
    directory = root / name
    directory.mkdir()
    key = b"Ed" + stored_id + b"K" * 32
    pub = (
        "untrusted comment: minisign public key: "
        + display_id.hex().upper()
        + "\n"
        + base64.b64encode(key).decode()
        + "\n"
    )
    (root / "pub.json").write_text(
        json.dumps({"plugins": {"updater": {"pubkey": base64.b64encode(pub.encode()).decode()}}}),
        encoding="utf-8",
    )

    sig = b"Ed" + signature_id + b"S" * 64
    trusted = "timestamp:0\tfile:fixture"
    if version is not None:
        trusted += f"\tversion:{version}"
    sig_text = (
        "untrusted comment: signature from tauri secret key\n"
        + base64.b64encode(sig).decode()
        + f"\ntrusted comment: {trusted}\n"
        + base64.b64encode(b"G" * 64).decode()
        + "\n"
    )
    sig_data = base64.b64encode(sig_text.encode()).decode() + "\n" if wrapped else sig_text

    if mac:
        (directory / "ice-box.app.tar.gz").write_text("darwin\n", encoding="utf-8")
        (directory / "ice-box.app.tar.gz.sig").write_text(sig_data, encoding="utf-8")
    (directory / "ice-box_0.2.6_x64-setup.exe").write_text("windows\n", encoding="utf-8")
    (directory / "ice-box_0.2.6_x64-setup.exe.sig").write_text(sig_data, encoding="utf-8")
    (directory / "ice-box_0.2.6_amd64.AppImage").write_text("linux-x64\n", encoding="utf-8")
    (directory / "ice-box_0.2.6_amd64.AppImage.sig").write_text(sig_data, encoding="utf-8")
    (directory / "ice-box_0.2.6_aarch64.AppImage").write_text("linux-arm64\n", encoding="utf-8")
    (directory / "ice-box_0.2.6_aarch64.AppImage.sig").write_text(sig_data, encoding="utf-8")
    return directory


write_fixture("ok", stored_id)
write_fixture("plain", stored_id, wrapped=False)
write_fixture("no-version", stored_id, version=None)
write_fixture("mismatch", bytes.fromhex("1111111111111111"))
missing = write_fixture("missing", stored_id)
(missing / "ice-box.app.tar.gz.sig").unlink()
missing_linux = write_fixture("missing-linux", stored_id)
(missing_linux / "ice-box_0.2.6_aarch64.AppImage.sig").unlink()
malformed = write_fixture("malformed", stored_id)
(malformed / "ice-box_0.2.6_x64-setup.exe.sig").write_text(
    "untrusted comment: signature\nnot-base64\n", encoding="utf-8"
)
(malformed / "ice-box_0.2.6_x64-setup.exe").write_text("windows\n", encoding="utf-8")
PY

expect_ok() {
  local name="$1"
  shift
  if ! "$SCRIPT" --pubkey-file "$TMP/pub.json" "$@" >/dev/null 2>&1; then
    echo "test-verify-updater-signatures: $name: expected success" >&2
    "$SCRIPT" --pubkey-file "$TMP/pub.json" "$@" >&2 || true
    exit 1
  fi
}

expect_fail() {
  local name="$1"
  local needle="$2"
  shift 2
  local err
  if err="$("$SCRIPT" --pubkey-file "$TMP/pub.json" "$@" 2>&1)"; then
    echo "test-verify-updater-signatures: $name: expected failure" >&2
    exit 1
  fi
  if ! printf '%s\n' "$err" | grep -q "$needle"; then
    echo "test-verify-updater-signatures: $name: missing '$needle' in:" >&2
    printf '%s\n' "$err" >&2
    exit 1
  fi
}

echo "== happy path =="
expect_ok "matching-key" "$TMP/ok"
expect_ok "matching-version" --version 0.2.6 "$TMP/ok"
expect_ok "matching-v-tag-version" --version v0.2.6 "$TMP/ok"
expect_ok "plain-minisign-text" "$TMP/plain"
expect_ok "no-version-without-flag" "$TMP/no-version"

echo "== failures =="
expect_fail "mismatched-key" "does not match" "$TMP/mismatch"
expect_fail "mismatched-version" "signed version '0.2.6' does not match" --version 0.2.7 "$TMP/ok"
expect_fail "missing-signed-version" "trusted comment has no version field" --version 0.2.6 "$TMP/no-version"
expect_fail "missing-macos-sig" "expected exactly one macOS" "$TMP/missing"
expect_fail "missing-linux-sig" "expected exactly one Linux arm64" "$TMP/missing-linux"
expect_fail "malformed-sig" "invalid base64" "$TMP/malformed"

echo "== strict signature verification =="
if command -v minisign >/dev/null 2>&1; then
  mkdir -p "$TMP/strict"
  minisign -G -p "$TMP/strict.pub" -s "$TMP/strict.key" -W >/dev/null
  printf 'darwin\n' >"$TMP/strict/ice-box.app.tar.gz"
  printf 'windows\n' >"$TMP/strict/ice-box_0.2.6_x64-setup.exe"
  printf 'linux-x64\n' >"$TMP/strict/ice-box_0.2.6_amd64.AppImage"
  printf 'linux-arm64\n' >"$TMP/strict/ice-box_0.2.6_aarch64.AppImage"
  minisign -S -s "$TMP/strict.key" -t $'timestamp:0\tfile:ice-box.app.tar.gz\tversion:0.2.6' \
    -m "$TMP/strict/ice-box.app.tar.gz" -x "$TMP/strict/ice-box.app.tar.gz.sig" >/dev/null
  minisign -S -s "$TMP/strict.key" -t $'timestamp:0\tfile:ice-box_0.2.6_x64-setup.exe\tversion:0.2.6' \
    -m "$TMP/strict/ice-box_0.2.6_x64-setup.exe" -x "$TMP/strict/ice-box_0.2.6_x64-setup.exe.sig" >/dev/null
  minisign -S -s "$TMP/strict.key" -t $'timestamp:0\tfile:ice-box_0.2.6_amd64.AppImage\tversion:0.2.6' \
    -m "$TMP/strict/ice-box_0.2.6_amd64.AppImage" -x "$TMP/strict/ice-box_0.2.6_amd64.AppImage.sig" >/dev/null
  minisign -S -s "$TMP/strict.key" -t $'timestamp:0\tfile:ice-box_0.2.6_aarch64.AppImage\tversion:0.2.6' \
    -m "$TMP/strict/ice-box_0.2.6_aarch64.AppImage" -x "$TMP/strict/ice-box_0.2.6_aarch64.AppImage.sig" >/dev/null
  if ! "$SCRIPT" --strict --version 0.2.6 --pubkey-file "$TMP/strict.pub" "$TMP/strict" >/dev/null; then
    fail "strict-valid: expected success"
  fi
  if "$SCRIPT" --strict --version 0.2.7 --pubkey-file "$TMP/strict.pub" "$TMP/strict" \
    >/dev/null 2>&1; then
    fail "strict-wrong-version: expected failure"
  fi
  cp -R "$TMP/strict" "$TMP/strict-tampered"
  printf 'x' >>"$TMP/strict-tampered/ice-box.app.tar.gz"
  if "$SCRIPT" --strict --pubkey-file "$TMP/strict.pub" "$TMP/strict-tampered" \
    >/dev/null 2>&1; then
    fail "strict-tampered: expected failure"
  fi
  echo "strict: valid signature accepted, tampered artifact rejected"
else
  echo "strict: skipped (minisign not installed)"
fi

echo "test-verify-updater-signatures: OK"
