#!/usr/bin/env bash
# Synthesize Tauri updater latest.json from signed release artifacts.
# Usage: scripts/merge-updater-latest.sh <assets-dir> <tag> [notes-file]
#
# Required artifacts in <assets-dir>:
#   *.app.tar.gz + *.app.tar.gz.sig   (darwin-aarch64)
#   *-setup.exe + *-setup.exe.sig     (windows-x86_64; falls back to a single *.exe)
#
# The signature fields are the full .sig file text (not a path). URLs point at
# GitHub Release assets: https://github.com/<repo>/releases/download/<tag>/<file>
#
# Env:
#   GITHUB_REPOSITORY          owner/repo (default: yilong-musk/ice-box)
#   ICE_BOX_UPDATER_PUB_DATE   RFC 3339 timestamp (default: now UTC)
set -euo pipefail

ASSETS="${1:-}"
TAG="${2:-}"
NOTES_FILE="${3:-}"

if [[ -z "$ASSETS" || -z "$TAG" ]]; then
  echo "usage: $0 <assets-dir> <tag> [notes-file]" >&2
  exit 1
fi

if [[ ! -d "$ASSETS" ]]; then
  echo "assets dir not found: $ASSETS" >&2
  exit 1
fi

if [[ -n "$NOTES_FILE" && ! -f "$NOTES_FILE" ]]; then
  echo "notes file not found: $NOTES_FILE" >&2
  exit 1
fi

if ! command -v python3 >/dev/null 2>&1; then
  echo "python3 is required to emit latest.json" >&2
  exit 1
fi

REPO="${GITHUB_REPOSITORY:-yilong-musk/ice-box}"
PUB_DATE="${ICE_BOX_UPDATER_PUB_DATE:-$(date -u +"%Y-%m-%dT%H:%M:%SZ")}"

python3 - "$ASSETS" "$TAG" "$NOTES_FILE" "$REPO" "$PUB_DATE" <<'PY'
import json
import sys
from pathlib import Path

assets = Path(sys.argv[1])
tag = sys.argv[2]
notes_file = sys.argv[3]
repo = sys.argv[4]
pub_date = sys.argv[5]


def fail(message: str) -> None:
    print(message, file=sys.stderr)
    sys.exit(1)


def read_signature(artifact: Path) -> str:
    sig = Path(str(artifact) + ".sig")
    if not sig.is_file():
        fail(f"missing signature file: {sig.name}")
    text = sig.read_text(encoding="utf-8")
    if not text.strip():
        fail(f"empty signature file: {sig.name}")
    return text


def pick_one(files: list[Path], label: str) -> Path:
    if len(files) == 0:
        fail(f"missing {label} artifact in {assets}")
    if len(files) > 1:
        names = ", ".join(p.name for p in files)
        fail(f"expected exactly one {label} artifact, found: {names}")
    return files[0]


darwin = pick_one(
    sorted(p for p in assets.glob("*.app.tar.gz") if p.is_file()),
    "darwin *.app.tar.gz",
)
windows = sorted(p for p in assets.glob("*-setup.exe") if p.is_file())
if not windows:
    windows = sorted(
        p
        for p in assets.glob("*.exe")
        if p.is_file() and not p.name.endswith(".exe.sig")
    )
windows_exe = pick_one(windows, "windows NSIS *.exe")

notes = ""
if notes_file:
    notes = Path(notes_file).read_text(encoding="utf-8").strip()

version = tag[1:] if tag.startswith(("v", "V")) else tag
if not version:
    fail(f"tag {tag!r} does not contain a version")

darwin_sig = read_signature(darwin)
windows_sig = read_signature(windows_exe)
base = f"https://github.com/{repo}/releases/download/{tag}"

payload = {
    "version": version,
    "notes": notes,
    "pub_date": pub_date,
    "platforms": {
        "darwin-aarch64": {
            "signature": darwin_sig,
            "url": f"{base}/{darwin.name}",
        },
        "windows-x86_64": {
            "signature": windows_sig,
            "url": f"{base}/{windows_exe.name}",
        },
    },
}

print(json.dumps(payload, indent=2, ensure_ascii=False))
PY
