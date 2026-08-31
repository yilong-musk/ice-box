#!/usr/bin/env bash
# Install the ice-box privileged helper as a launchd daemon (plan §5 T5).
#
# Thin wrapper over the helper's own privileged `install` mode: the install
# logic (token, plist, ownership, pinned SHA-256, launchctl) lives in
# crates/ice-helper/src/install.rs and is shared with the in-app installer
# (system authorization dialog). This script is the explicit manual/CI opt-in
# that runs the same mode through sudo.
#
# Security model: the helper executes the core binary as root, so the
# installer copies it into a root-owned location (never executed from a
# user-writable path), refuses a group/world-writable source, and pins the
# binary's SHA-256 in the launchd plist. The daemon refuses to start when
# the on-disk binary does not match the pinned hash.
#
# Usage:
#   ./scripts/install-helper-macos.sh [DATA_DIR] [CORE_BIN] [HELPER_BIN]
#
# Defaults:
#   DATA_DIR  = $HOME/Library/Application Support/com.yilong-musk.icebox
#   CORE_BIN  = path of the bundled sing-box (first existing candidate)
#   HELPER_BIN = the binary to run elevated (dev fallback: build locally)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "install-helper-macos.sh is macOS-only" >&2
  exit 1
fi

echo "== preflight: sudo =="
sudo -v

DATA_DIR="${1:-$HOME/Library/Application Support/com.yilong-musk.icebox}"
CORE_BIN="${2:-}"
HELPER_SRC="${3:-${ICE_BOX_HELPER_BIN:-}}"

if [[ -z "$HELPER_SRC" && -x "$ROOT/apps/desktop/src-tauri/resources/ice-helper" ]]; then
  HELPER_SRC="$ROOT/apps/desktop/src-tauri/resources/ice-helper"
fi

if [[ -z "$CORE_BIN" ]]; then
  for candidate in \
    "$ROOT/third_party/sing-box/darwin-aarch64/sing-box" \
    "$ROOT/third_party/sing-box/darwin-x86_64/sing-box" \
    "$ROOT/apps/desktop/src-tauri/resources/sing-box"
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
  echo "ERROR: data dir not found: $DATA_DIR" >&2
  exit 1
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
echo "  binary : /Library/PrivilegedHelperTools/com.yilong-musk.icebox.helper"
echo "  plist  : /Library/LaunchDaemons/com.yilong-musk.icebox.helper.plist"
echo "  socket : /var/run/ice-box-helper.sock"
echo "  token  : $DATA_DIR/helper-token (root:wheel 0644)"
echo ""
echo "To uninstall: sudo ./scripts/uninstall-helper-macos.sh \"$DATA_DIR\""