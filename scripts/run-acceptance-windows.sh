#!/usr/bin/env bash
# Run Windows acceptance (Git Bash): automated gate + headless G9 + live (--ignored) tests.
# Live steps mutate the real WinInet Internet Settings and spawn real sing-box;
# run only on a Windows host you are willing to let the script restore.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "========== G9.10 workspace gate =========="
bash scripts/gate.sh

echo ""
echo "========== G9.1 / G9.6 / G9.7 (headless) =========="
cargo test -p ice-box --lib 'g9_' -- --nocapture

if [[ "${OS:-}" != "Windows_NT" ]]; then
  echo "Skip live Windows tests (not Windows)"
  exit 0
fi

if [[ ! -x third_party/sing-box/windows-x86_64/sing-box.exe ]]; then
  echo "ERROR: sing-box.exe missing — run ./scripts/fetch-singbox.sh win" >&2
  exit 1
fi

echo ""
echo "========== G4.3-windows then G4.4-windows (WinInet roundtrip) =========="
cargo test -p ice-proxy-sys g4_3 -- --ignored --nocapture
cargo test -p ice-proxy-sys g4_4 -- --ignored --nocapture

echo ""
echo "========== G9 live (sing-box + mode switching via Clash API) =========="
cargo test -p ice-box --lib 'live::' -- --ignored --nocapture

echo ""
echo "========== G4.3-windows restore check (after live tests) =========="
cargo test -p ice-proxy-sys g4_3 -- --ignored --nocapture

echo ""
echo "========== Manual only (not automated) =========="
echo "  Installer: npm run build:win → NSIS; sing-box.exe must land in the install dir"
echo "  Tray / 关窗隐藏 — npm run dev:win 后目视确认"
echo "  System proxy on/off from UI; mode switch while downloading"
echo ""
echo "Windows acceptance run: OK (automated + live)"