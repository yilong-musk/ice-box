#!/usr/bin/env bash
# Run macOS acceptance: automated gate + headless G9 + live (--ignored) tests.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "========== G9.10 workspace gate =========="
bash scripts/gate.sh

echo ""
echo "========== G9.1 / G9.6 / G9.7 (headless) =========="
cargo test -p ice-box --lib 'g9_' -- --nocapture

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "Skip live macOS tests (not Darwin)"
  exit 0
fi

if [[ ! -x third_party/sing-box/darwin-aarch64/sing-box && ! -x third_party/sing-box/darwin-x86_64/sing-box ]]; then
  echo "ERROR: sing-box binary missing — run ./scripts/fetch-singbox.sh" >&2
  exit 1
fi

echo ""
echo "========== G4.3 then G4.4 (proxy roundtrip) =========="
cargo test -p ice-proxy-sys g4_3 -- --ignored --nocapture
cargo test -p ice-proxy-sys g4_4 -- --ignored --nocapture

echo ""
echo "========== G9 live (sing-box + proxy) =========="
cargo test -p ice-box --lib 'live::' -- --ignored --nocapture

echo ""
echo "========== G4.3 restore check (after live tests) =========="
cargo test -p ice-proxy-sys g4_3 -- --ignored --nocapture

echo ""
echo "========== Manual only (not automated) =========="
echo "  G9.9  关窗隐藏 / 托盘退出 — 需 GUI，请 npm run dev 后目视确认"
echo "  G8.4  安装包 .app 启动 — open target/release/bundle/macos/ice-box.app"
echo ""
echo "macOS acceptance run: OK (automated + live)"
