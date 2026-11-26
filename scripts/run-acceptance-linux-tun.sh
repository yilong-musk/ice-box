#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Linux TUN live acceptance (`docs/testing.md`).
#
# Two runner modes:
#   1. Dev `sudo` runner (default): runs sing-box elevated through `sudo -n`
#      (requires cached credential via `sudo -v` or NOPASSWD rule).
#   2. Helper path (`--helper`): installs the systemd helper via
#      `scripts/install-helper-linux.sh`, runs enable -> traffic -> disable
#      through helper IPC (`g9_13`), then uninstalls cleanly.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

MODE="sudo"
if [[ "${1:-}" == "--helper" ]]; then
  MODE="helper"
fi

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "Linux TUN live acceptance is Linux-only" >&2
  exit 1
fi

if [[ ! -x third_party/sing-box/linux-x86_64/sing-box && ! -x third_party/sing-box/linux-aarch64/sing-box ]]; then
  echo "ERROR: sing-box binary missing — run ./scripts/fetch-singbox.sh" >&2
  exit 1
fi

if [[ "$MODE" == "sudo" ]]; then
  echo "== preflight: sudo -n (cached credential or NOPASSWD) =="
  if ! sudo -n true 2>/dev/null; then
    cat >&2 <<'EOF'
ERROR: the dev sudo runner needs root without an interactive prompt.
Run `sudo -v` once in this terminal (or add a NOPASSWD rule) and retry.
EOF
    exit 1
  fi

  echo ""
  echo "== G9.12-linux: TUN enable -> mixed curl -> disable -> adapter removal =="
  ICE_BOX_TUN_DEV_SUDO=1 cargo test -p ice-box --lib g9_12 -- --ignored --nocapture
else
  echo "== installing the Linux systemd helper =="
  DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/com.yilong-musk.icebox"
  mkdir -p "$DATA_DIR"
  cleanup_helper() {
    echo ""
    echo "== uninstalling the Linux systemd helper (cleanup) =="
    bash scripts/uninstall-helper-linux.sh "$DATA_DIR" || true
  }
  trap cleanup_helper EXIT

  bash scripts/install-helper-linux.sh "$DATA_DIR"

  echo ""
  echo "== G9.13-linux: TUN enable/disable through the systemd helper IPC =="
  ICE_BOX_TUN_LIVE_DATA_DIR="$DATA_DIR" \
  cargo test -p ice-box --lib g9_13 -- --ignored --nocapture

  trap - EXIT
  echo ""
  echo "== uninstalling the Linux systemd helper =="
  bash scripts/uninstall-helper-linux.sh "$DATA_DIR"
fi

echo ""
echo "Linux TUN acceptance: OK (mode: $MODE)"
