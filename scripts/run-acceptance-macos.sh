#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
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
echo "  G9.9  关窗隐藏 / 托盘退出 / 托盘菜单代理开关、模式切换、订阅切换与节点切换（多层子菜单）/ 图标右侧的实时速度读数（上下两行，代理服务运行时每秒刷新；关闭后显示 0.0 而不消失并转为淡灰色，仅内核单独运行时同样置灰；读数不补前导零，宽度恒定不抖动）/ 设置里的「系统托盘图标」三种显示模式（图标+网速、仅图标、仅网速）即时生效并持久化 — 需 GUI，请 npm run dev 后目视确认"
echo "  G8.4  安装包 .app 启动 — open target/release/bundle/macos/ice-box.app"
echo ""
echo "macOS acceptance run: OK (automated + live)"
