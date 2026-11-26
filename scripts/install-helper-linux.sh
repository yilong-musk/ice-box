#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Install the ice-box privileged helper as a systemd daemon (`docs/tun.md`).
#
# Thin wrapper over the helper's own privileged `install` mode: the install
# logic (token, systemd unit, ownership, pinned SHA-256, geoip copy, systemctl)
# lives in crates/ice-helper/src/install.rs and is shared with the in-app
# installer (Polkit `pkexec` dialog). This script is the explicit manual/CI
# opt-in that runs the same mode through sudo.
#
# Usage:
#   ./scripts/install-helper-linux.sh [DATA_DIR] [CORE_BIN] [HELPER_BIN]
#
# Defaults:
#   DATA_DIR   = ${XDG_DATA_HOME:-$HOME/.local/share}/com.yilong-musk.icebox
#   CORE_BIN   = path of the bundled sing-box (first existing candidate)
#   HELPER_BIN = the binary to run elevated (dev fallback: build locally)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "install-helper-linux.sh is Linux-only" >&2
  exit 1
fi

if [[ ! -d /run/systemd/system ]]; then
  echo "ERROR: systemd is not running (/run/systemd/system missing)" >&2
  exit 1
fi

echo "== preflight: sudo =="
sudo -v

DEFAULT_DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/com.yilong-musk.icebox"
DATA_DIR="${1:-$DEFAULT_DATA_DIR}"
CORE_BIN="${2:-}"
HELPER_SRC="${3:-${ICE_BOX_HELPER_BIN:-}}"

if [[ -z "$HELPER_SRC" && -x "$ROOT/apps/desktop/src-tauri/resources/ice-helper" ]]; then
  HELPER_SRC="$ROOT/apps/desktop/src-tauri/resources/ice-helper"
fi

if [[ -z "$CORE_BIN" ]]; then
  for candidate in \
    "$ROOT/apps/desktop/src-tauri/resources/sing-box" \
    "$ROOT/third_party/sing-box/linux-x86_64/sing-box" \
    "$ROOT/third_party/sing-box/linux-aarch64/sing-box"
  do
    if [[ -x "$candidate" ]]; then
      CORE_BIN="$candidate"
      break
    fi
  done
fi
if [[ -z "$CORE_BIN" || ! -x "$CORE_BIN" ]]; then
  echo "ERROR: sing-box binary not found — pass it as the 2nd argument or run scripts/fetch-singbox.sh" >&2
  exit 1
fi
if [[ ! -d "$DATA_DIR" ]]; then
  mkdir -p "$DATA_DIR"
fi

if [[ -z "$HELPER_SRC" ]]; then
  echo "== building ice-helper =="
  cargo build --release -p ice-helper --manifest-path "$ROOT/Cargo.toml"
  HELPER_SRC="$ROOT/target/release/ice-helper"
else
  echo "== using supplied ice-helper =="
fi
if [[ ! -x "$HELPER_SRC" ]]; then
  echo "ERROR: ice-helper binary not built at $HELPER_SRC" >&2
  exit 1
fi

echo "== running ice-helper install (root) =="
sudo "$HELPER_SRC" install "$DATA_DIR" "$CORE_BIN" "$(id -u)"

echo ""
echo "ice-helper installed:"
echo "  binary    : /usr/local/libexec/ice-box/ice-helper"
echo "  core      : /usr/local/libexec/ice-box/sing-box"
echo "  resources : /usr/local/libexec/ice-box/resources"
echo "  unit      : /etc/systemd/system/ice-box-helper.service"
echo "  socket    : /run/ice-box-helper.sock"
echo "  token     : $DATA_DIR/helper-token (installing uid, 0600)"
echo ""
echo "To uninstall: sudo ./scripts/uninstall-helper-linux.sh \"$DATA_DIR\""
