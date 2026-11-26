#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Run Linux acceptance: automated gate + headless G9 + live (--ignored) tests.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "========== G9.10 workspace gate =========="
bash scripts/gate.sh

echo ""
echo "========== G9.1 / G9.6 / G9.7 (headless) =========="
cargo test -p ice-box --lib 'g9_' -- --nocapture

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "Skip live Linux tests (not Linux)"
  exit 0
fi

if [[ ! -x third_party/sing-box/linux-x86_64/sing-box && ! -x third_party/sing-box/linux-aarch64/sing-box ]]; then
  echo "ERROR: sing-box binary missing — run ./scripts/fetch-singbox.sh" >&2
  exit 1
fi

if [[ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}${DBUS_SESSION_BUS_ADDRESS:-}" ]]; then
  echo ""
  echo "========== G4.3 then G4.4 (Linux desktop proxy roundtrip) =========="
  cargo test -p ice-proxy-sys g4_3 -- --ignored --nocapture
  cargo test -p ice-proxy-sys g4_4 -- --ignored --nocapture
else
  echo "Skip G4.3/G4.4 live desktop proxy tests (no graphical/D-Bus session)"
fi

echo ""
echo "========== G9 live (sing-box + mixed) =========="
cargo test -p ice-box --lib 'live::' -- --ignored --nocapture

echo ""
echo "========== Manual only (not automated) =========="
echo "  - Desktop tray icon / menu actions / window hide-reopen via single-instance"
echo "  - .deb install (sudo apt install ./ice-box_*.deb) -> verify Update shows Download"
echo "  - .AppImage launch -> verify Launch at login writes \$APPIMAGE to ~/.config/autostart/"
echo ""
echo "Linux acceptance run: OK (automated + live)"
