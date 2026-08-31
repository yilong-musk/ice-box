#!/usr/bin/env bash
# macOS TUN live acceptance (plan §6 "Live macOS acceptance").
#
# Two runner modes:
#
# 1. Dev `sudo` runner (plan §5 T3 exit gate / macOS live gate): the
#    bundled sing-box runs elevated through `sudo -n`, so this mode
#    requires a cached root credential (`sudo -v` in a terminal) or a
#    NOPASSWD rule. No interactive password prompt is issued by the test
#    itself.
#
# 2. Helper path (plan §5 T5): `--helper` installs the helper
#    daemon (scripts/install-helper-macos.sh), then runs the same enable →
#    traffic → disable roundtrip through the helper IPC, then uninstalls it.
#
# The ordinary gates (scripts/gate-local.sh / gate.sh) stay non-privileged
# and never mutate host routes or proxy settings; this script is the
# explicit, destructive live suite.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

MODE="sudo"
if [[ "${1:-}" == "--helper" ]]; then
  MODE="helper"
fi

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "TUN live acceptance is macOS-only (Windows gate pending)" >&2
  exit 1
fi

if [[ ! -x third_party/sing-box/darwin-aarch64/sing-box && ! -x third_party/sing-box/darwin-x86_64/sing-box ]]; then
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
  echo "== G9.12: TUN enable -> mixed curl -> disable -> adapter removal =="
  ICE_BOX_TUN_DEV_SUDO=1 cargo test -p ice-box --lib g9_12 -- --ignored --nocapture
else
  echo "== installing the privileged helper (T5) =="
  DATA_DIR="$HOME/Library/Application Support/com.yilong-musk.icebox"
  mkdir -p "$DATA_DIR"
  cleanup_helper() {
    echo ""
    echo "== uninstalling the privileged helper (cleanup) =="
    bash scripts/uninstall-helper-macos.sh "$DATA_DIR" || true
  }
  trap cleanup_helper EXIT

  bash scripts/install-helper-macos.sh "$DATA_DIR"

  echo ""
  echo "== G9.13: TUN enable/disable through the helper IPC =="
  ICE_BOX_TUN_LIVE_DATA_DIR="$DATA_DIR" \
  cargo test -p ice-box --lib g9_13 -- --ignored --nocapture

  trap - EXIT
  echo ""
  echo "== uninstalling the privileged helper =="
  bash scripts/uninstall-helper-macos.sh "$DATA_DIR"
fi

echo ""
echo "macOS TUN acceptance: OK (mode: $MODE)"
