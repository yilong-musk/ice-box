#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Uninstall the ice-box Linux privileged helper (`docs/tun.md`). Explicit opt-in,
# requires root. Thin wrapper over the helper's privileged `uninstall` mode
# (crates/ice-helper/src/install.rs, shared with the in-app Polkit dialog).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEFAULT_DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/com.yilong-musk.icebox"
DATA_DIR="${1:-$DEFAULT_DATA_DIR}"
INSTALLED_HELPER="/usr/local/libexec/ice-box/ice-helper"

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "uninstall-helper-linux.sh is Linux-only" >&2
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
  "/etc/systemd/system/ice-box-helper.service" \
  "/usr/local/libexec/ice-box/ice-helper" \
  "/usr/local/libexec/ice-box/sing-box" \
  "/usr/local/libexec/ice-box/resources" \
  "/var/log/ice-box-core.log" \
  "/var/log/ice-box-helper.log" \
  "/run/ice-box-helper.sock"; do
  if [[ -e "$path" ]]; then
    echo "ERROR: helper residue remains: $path" >&2
    exit 1
  fi
done
if systemctl is-active --quiet ice-box-helper.service 2>/dev/null; then
  echo "ERROR: systemd helper is still active" >&2
  exit 1
fi
if [[ -e "$DATA_DIR/helper-token" ]]; then
  echo "ERROR: helper token remains: $DATA_DIR/helper-token" >&2
  exit 1
fi

echo "ice-helper uninstalled"
