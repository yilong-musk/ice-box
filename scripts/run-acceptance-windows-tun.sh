#!/usr/bin/env bash
# Windows TUN live acceptance (plan §6 "Live Windows acceptance").
#
# The Windows T0 spike (`windows_tun_ready`) is still pending; this script is
# the dev opt-in runner that turns the host spike into a repeatable gate. It
# exercises the native-path enable -> traffic -> disable roundtrip on a real
# Windows host:
#
#   1. Preflights elevation: the wintun driver is embedded in the bundled
#      sing-box binary, and adapter creation needs an Administrator context.
#      Run this script from an elevated shell (or `Start-Process -Verb RunAs`
#      a PowerShell window first) — no UAC prompt is issued by the test itself.
#   2. Runs G9.14 with ICE_BOX_TUN_WINDOWS_DEV=1 (the dev elevated runner).
#
# The ordinary gates (scripts/gate-local.sh / gate.sh) stay non-privileged
# and never mutate host routes or proxy settings; this script is the
# explicit, destructive live suite.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if [[ "$(uname -s)" != MINGW* && "$(uname -s)" != MSYS* && "$(uname -s)" != CYGWIN* && "${OS:-}" != "Windows_NT" ]]; then
  echo "Windows TUN live acceptance must run on a Windows host" >&2
  exit 1
fi

if [[ ! -x third_party/sing-box/windows-x86_64/sing-box.exe ]]; then
  echo "ERROR: sing-box binary missing — run ./scripts/fetch-singbox.sh win" >&2
  exit 1
fi

echo "== preflight: elevated context (wintun adapter creation needs admin) =="
if ! net session >/dev/null 2>&1; then
  cat >&2 <<'EOF'
ERROR: this shell is not elevated. Reopen the terminal as Administrator
(UAC) and retry — the test never issues a UAC prompt itself.
EOF
  exit 1
fi

echo ""
echo "== G9.14: TUN enable -> mixed curl -> disable -> adapter removal =="
ICE_BOX_TUN_WINDOWS_DEV=1 cargo test -p ice-box --lib g9_14 -- --ignored --nocapture

echo ""
echo "Windows TUN acceptance: OK (dev elevated runner)"