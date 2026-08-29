# ice-box

A lightweight network proxy client for macOS / Windows: Tauri 2 + React shell with a **sing-box** core, supporting subscription import and management.

## License

ice-box is released under the [MIT License](LICENSE).

Bundled **sing-box** remains under its upstream GPL-3.0-or-later terms (with the
upstream naming restriction). See [NOTICE](NOTICE) and
[third_party/sing-box/LICENSE](third_party/sing-box/LICENSE).

## Architecture overview

The implementation spec lives in [docs/architecture.md](docs/architecture.md) (process model, state machine, single active subscription, IPC, failure rollback).

- Traffic capture (system proxy or **TUN**): system proxy on macOS/Windows; TUN is a Settings switch — when enabled, the Home proxy-service button starts transparent capture through a sing-box `tun` inbound instead of changing the OS proxy (see [docs/tun-mode-plan.md](docs/tun-mode-plan.md) for the status/slices)
- Subscriptions: sing-box first, Clash compatible; **only one subscription is active at a time**, switching the active subscription switches the entire set of policy groups / rules / DNS
- Runtime updates: hot reload first (**SIGHUP** in-process rebuild, PID unchanged), restart as fallback
- **Modes**: one-click switch between Rule / Global / Direct from the home page, switched **live via the Clash API** (`PATCH /configs`) on macOS **and** Windows — no config rebuild, no reload, no core restart, no connection drop
- **Nodes**: switch outbound and run Clash API latency tests
- **Rules**: query / search / filter by type / pagination (server-side filtering, tens of thousands of rules never cross IPC as a full table), disable / enable (fingerprint persisted), custom rules
- **Traffic**: real-time upload/download chart on the home page (Clash API `/traffic` persistent stream + 60s ring buffer)

## Repository layout

```
apps/desktop          # Desktop app
crates/ice-core       # Core lifecycle
crates/ice-proxy-sys  # System proxy
crates/ice-config     # Config generation
crates/ice-subscription
third_party/sing-box  # Binary placement notes
configs/examples
```

## Development

Prerequisites: Rust (stable), Node.js, (macOS) Xcode CLT.

```bash
# Frontend dependencies (first time)
cd apps/desktop && npm install && cd ../..

# Check the Rust workspace from the repo root
cargo check

# Run the desktop app (host platform by default)
npm run dev

# Explicitly target a platform (useful for cross-architecture macOS development)
npm run dev:mac-arm64   # Apple Silicon
npm run dev:mac-x64     # Intel Mac
npm run dev:win         # Windows (must run on a Windows host)
```

## sing-box binaries

Platform binaries live in `third_party/sing-box/` (not committed):

```
darwin-aarch64/sing-box
darwin-x86_64/sing-box
windows-x86_64/sing-box.exe
```

**Bundled pin:** see `third_party/sing-box/VERSION` (currently **1.13.19**). Official releases: [sing-box releases](https://github.com/SagerNet/sing-box/releases). The pinned version is also recorded in `docs/architecture.md` §4.3 / §21.

### Fetch (recommended)

From the repo root:

```bash
npm run fetch-singbox              # current host
npm run fetch-singbox -- mac-x64   # specific platform
./scripts/prepare-singbox-resource.sh   # copies into apps/desktop/src-tauri/resources/
```

### Manual (Apple Silicon example)

```bash
VER=$(tr -d '[:space:]' < third_party/sing-box/VERSION)
curl -fsSL -o /tmp/sb.tgz \
  "https://github.com/SagerNet/sing-box/releases/download/v${VER}/sing-box-${VER}-darwin-arm64.tar.gz"
tar -xzf /tmp/sb.tgz -C /tmp
mkdir -p third_party/sing-box/darwin-aarch64
cp /tmp/sing-box-${VER}-darwin-arm64/sing-box third_party/sing-box/darwin-aarch64/
chmod +x third_party/sing-box/darwin-aarch64/sing-box
./third_party/sing-box/darwin-aarch64/sing-box version
```

`npm run build` / `tauri build` runs `scripts/prepare-singbox-resource.sh` first, bundling the current-platform binary into the installer resources.

## Error shape (locked)

Tauri commands uniformly return JSON on failure:

```json
{ "code": "config.empty_outbounds", "message": "human readable detail" }
```

- `code`: stable strings listed in the architecture [§17](docs/architecture.md) (e.g. `core.not_found`, `sub.fetch_failed`)
- `message`: explanation shown to the UI
- Rust type: `ice_config::AppError`

## Status

**macOS v2 routing capabilities** (2026-08-22): v1 slices 0-9 have passed the gate; **single active subscription** replaces multi-subscription merging (breaking change, see architecture §11.5); Clash `proxy-groups` → selector/urltest/fallback/loadbalance, `rules` → route.rules (**GEOIP → bundled sing-geoip rule-sets**, 30 countries, see `scripts/fetch-geoip.sh`), subscription `dns` → sing-box dns (listen stripped); bundled sing-box **1.13.19**; `.app` / `.dmg` buildable.

**Windows completion** (2026-08-24, see [docs/windows-plan.md](docs/windows-plan.md)): WinInet system proxy implemented (`ice-proxy-sys` Windows backend, per-user `Internet Settings`, no elevation); mode switching rebuilds + reloads the core on both platforms (SIGHUP on macOS, restart on Windows — the Clash API `PATCH /configs` fast path is a forward-compatible gate that the pinned sing-box never honors); Windows CI runner (`gate-windows`), NSIS packaging (`npm run build:win`) and Windows acceptance (`npm run acceptance:win`) added.

| Scope | Status |
|-------|--------|
| macOS | ✅ Release gate passed |
| CI | `.github/workflows/ci.yml` → Linux + macOS + **Windows** `npm run gate` |
| Windows | ✅ System proxy (WinInet), mode switching, NSIS build script, acceptance script |
| TUN | 🚧 Slices T0–T4 landed, T5 macOS helper + packaging landed (G9.12 + G9.13 live gates green; macOS release is permanently unsigned with in-app elevated helper installation; clean-machine gate waived for this release); Windows T0 pending — see [docs/tun-mode-plan.md](docs/tun-mode-plan.md) |

### TUN status and prerequisites

- The Settings page has a **TUN mode** switch; when enabled, the Home「启动代理服务」button starts transparent capture through a sing-box `tun` inbound instead of the OS system proxy. `system_proxy` and TUN stay mutually exclusive, and stopping the service disables whichever backend is active.
- macOS needs root for adapter/route changes. Two elevated-core runners exist:
  - **Production helper** (T5): a small `ice-helper` launchd daemon (root-owned socket + per-installation token; narrow start/stop IPC with path allowlist). The release is permanently unsigned, so on first use the app prompts the system authorization dialog and installs it itself (Settings → TUN 模式 →「安装辅助组件」); `scripts/install-helper-macos.sh` / `uninstall-helper-macos.sh` are the manual/CI equivalent. The install logic lives once in `crates/ice-helper/src/install.rs`.
  - **Dev runner**: the opt-in `ICE_BOX_TUN_DEV_SUDO=1` plus a cached root credential (`sudo -v`) runs the core via `sudo -n`. The destructive live suite is `npm run acceptance:tun` (macOS only; `--helper` runs it through the installed helper).
- Without the helper installed, TUN transitions fail closed with a permission error and no system change; the Home page then offers the in-app「安装辅助组件」action.
- A platform with a pending gate (`tun_available=false`, e.g. Windows) shows the switch disabled with the reason; the system-proxy fallback remains the documented path there.
- If TUN cleanup cannot be confirmed (e.g. crash), the app fail-closes (`recovery_required`), blocks new TUN activation, and Home offers「重试恢复」. Cleanup is ownership-verified from `tun-state.json`; unrelated routes/DNS are never touched.

### Common commands

```bash
npm run dev              # dev (current host)
npm run dev:mac-arm64    # dev (Apple Silicon)
npm run dev:mac-x64      # dev (Intel Mac)
npm run dev:win          # dev (Windows)
npm run build            # macOS .app / .dmg
npm run build:win        # Windows NSIS (Windows host, PowerShell fallback prepared)
npm run gate             # fmt + clippy + test + tsc + vitest
npm run acceptance       # full macOS acceptance (incl. live sing-box / system proxy)
npm run acceptance:tun   # macOS TUN live acceptance (destructive; sudo -v or helper; add --helper for the T5 daemon path)
npm run acceptance:win   # full Windows acceptance (incl. live WinInet / sing-box, Git Bash)
```

### macOS release (local build)

```bash
./scripts/fetch-singbox.sh
npm run build
open target/release/bundle/dmg/ice-box_*_aarch64.dmg   # or .app
```

### Windows release (local build)

```bash
./scripts/fetch-singbox.sh win
npm run build:win
# artifacts: apps/desktop/src-tauri/target/release/bundle/nsis/
```

## Release process

Releases are versioned with `vX.Y.Z` tags; the tag push triggers
`.github/workflows/release.yml` (gate + macOS arm64 dmg + Windows NSIS build +
GitHub Release with assets and `NOTICE`).

Quick reference:

```bash
bash scripts/bump-version.sh 0.1.2   # sync all three version sources
# update CHANGELOG.md, run gate-local.sh, merge to main via PR
git tag -a v0.1.2 -m "ice-box v0.1.2"
git push origin v0.1.2               # triggers the release pipeline
```

The full, step-by-step process (as executed for v0.1.1) is documented in
[docs/release-process.md](docs/release-process.md), including known issues
(network proxies, branch protection, CI warnings).
