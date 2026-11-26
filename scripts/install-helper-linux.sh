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

CURRENT_UID="$(id -u)"
if [[ "$CURRENT_UID" -eq 0 ]]; then
  if [[ -z "${SUDO_UID:-}" || "$SUDO_UID" -eq 0 ]]; then
    echo "ERROR: refusing to install directly as root without SUDO_UID (that would lock the helper socket to uid 0); run as your desktop user or via sudo" >&2
    exit 1
  fi
  TARGET_UID="$SUDO_UID"
else
  TARGET_UID="$CURRENT_UID"
  echo "== preflight: sudo =="
  sudo -v
fi

TARGET_HOME="$HOME"
if [[ -n "${SUDO_USER:-}" ]]; then
  SUDO_HOME="$(getent passwd "$SUDO_USER" | cut -d: -f6 || true)"
  if [[ -n "$SUDO_HOME" ]]; then
    TARGET_HOME="$SUDO_HOME"
  fi
fi

# shellcheck source=scripts/platform.sh
source "$ROOT/scripts/platform.sh"
ice_resolve_platform host

DEFAULT_DATA_DIR="${XDG_DATA_HOME:-$TARGET_HOME/.local/share}/com.yilong-musk.icebox"
DATA_DIR="${1:-$DEFAULT_DATA_DIR}"
CORE_BIN="${2:-}"
HELPER_SRC="${3:-${ICE_BOX_HELPER_BIN:-}}"

if [[ -z "$HELPER_SRC" && -x "$ROOT/apps/desktop/src-tauri/resources/ice-helper" ]]; then
  HELPER_SRC="$ROOT/apps/desktop/src-tauri/resources/ice-helper"
fi

if [[ -z "$CORE_BIN" ]]; then
  for candidate in \
    "$ROOT/apps/desktop/src-tauri/resources/sing-box" \
    "$ROOT/third_party/sing-box/$ICE_TARGET_DIR/sing-box"
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
CORE_BIN="$(cd "$(dirname "$CORE_BIN")" && pwd)/$(basename "$CORE_BIN")"
if [[ ! -d "$DATA_DIR" ]]; then
  if [[ "$CURRENT_UID" -eq 0 && -n "${SUDO_USER:-}" ]]; then
    sudo -u "$SUDO_USER" mkdir -p "$DATA_DIR"
  else
    mkdir -p "$DATA_DIR"
  fi
fi
DATA_DIR="$(cd "$DATA_DIR" && pwd)"

if [[ -z "$HELPER_SRC" ]]; then
  if [[ "$CURRENT_UID" -eq 0 ]]; then
    echo "ERROR: ice-helper binary not found and refusing to run 'cargo build' as root; run as your user or pass HELPER_BIN as the 3rd argument" >&2
    exit 1
  fi
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
if [[ "$CURRENT_UID" -eq 0 ]]; then
  "$HELPER_SRC" install "$DATA_DIR" "$CORE_BIN" "$TARGET_UID"
else
  sudo "$HELPER_SRC" install "$DATA_DIR" "$CORE_BIN" "$TARGET_UID"
fi

echo ""
echo "ice-helper installed:"
echo "  binary    : /usr/local/libexec/ice-box/ice-helper"
echo "  core      : /usr/local/libexec/ice-box/sing-box"
echo "  resources : /usr/local/libexec/ice-box/resources"
echo "  unit      : /etc/systemd/system/ice-box-helper.service"
echo "  env file  : /etc/ice-box/helper.env (root, 0600)"
echo "  socket    : /run/ice-box-helper.sock"
echo "  token     : $DATA_DIR/helper-token (uid $TARGET_UID, 0600)"
echo ""
echo "To uninstall: sudo ./scripts/uninstall-helper-linux.sh \"$DATA_DIR\""
