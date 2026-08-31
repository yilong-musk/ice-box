#!/usr/bin/env bash
# Uninstall the ice-box privileged helper (plan §5 T5). Explicit opt-in,
# requires root. Thin wrapper over the helper's privileged `uninstall` mode
# (crates/ice-helper/src/install.rs, shared with the in-app dialog); it
# removes the launchd daemon, the helper binary, and the per-installation
# token. Never touches routes / adapters / DNS — sing-box owns those and the
# app's recovery path cleans them.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DATA_DIR="${1:-$HOME/Library/Application Support/com.yilong-musk.icebox}"
INSTALLED_HELPER="/Library/PrivilegedHelperTools/com.yilong-musk.icebox.helper"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "uninstall-helper-macos.sh is macOS-only" >&2
  exit 1
fi

HELPER_SRC="$INSTALLED_HELPER"
if [[ ! -x "$HELPER_SRC" ]]; then
  if [[ -x "$ROOT/apps/desktop/src-tauri/resources/ice-helper" ]]; then
    HELPER_SRC="$ROOT/apps/desktop/src-tauri/resources/ice-helper"
  else
    echo "== building ice-helper =="
    cargo build --release -p ice-helper --manifest-path "$ROOT/Cargo.toml"
    HELPER_SRC="$ROOT/target/release/ice-helper"
  fi
fi

echo "== running ice-helper uninstall (root) =="
sudo "$HELPER_SRC" uninstall "$DATA_DIR"

echo "== verify helper residue is gone =="
for path in \
  "/Library/LaunchDaemons/com.yilong-musk.icebox.helper.plist" \
  "/Library/PrivilegedHelperTools/com.yilong-musk.icebox.helper" \
  "/Library/PrivilegedHelperTools/com.yilong-musk.icebox/sing-box" \
  "/var/log/ice-box-core.log" \
  "/var/log/ice-box-helper.log" \
  "/var/run/ice-box-helper.sock"; do
  if [[ -e "$path" ]]; then
    echo "ERROR: helper residue remains: $path" >&2
    exit 1
  fi
done
if launchctl print system/com.yilong-musk.icebox.helper >/dev/null 2>&1; then
  echo "ERROR: launchd helper is still loaded" >&2
  exit 1
fi
if [[ -e "$DATA_DIR/helper-token" ]]; then
  echo "ERROR: helper token remains: $DATA_DIR/helper-token" >&2
  exit 1
fi

echo "ice-helper uninstalled"