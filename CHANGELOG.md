# Changelog

All notable changes to this project are documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Changed

- ice-box is now licensed under GPL-3.0-or-later (previously MIT). `LICENSE`
  carries the GPLv3 text; `NOTICE`, the Cargo workspace, and the package
  manifests declare the new license, and every source file carries an
  `SPDX-License-Identifier: GPL-3.0-or-later` header. Releases up to 0.1.6
  stay MIT.
- README redesigned around the product (hero, install, quick start, TUN) with a
  Simplified Chinese edition in `README.zh-CN.md`; the website tagline is now
  "Proxy, kept simple."
- CI `scripts/gate.sh` runs `cargo test --workspace --exclude ice-box`
  (lib + integration + doc tests), so the TUN integration suite in
  `crates/ice-tun-sys/tests/` actually executes, then
  `cargo test -p ice-box --lib` for the Tauri desktop crate. The local
  pre-commit gate keeps `--lib` and adds `cargo test -p ice-tun-sys
  --tests`. The Windows CI job runs `ice-proxy-sys` tests.
- Config generation no longer uses `cfg(target_os)` inside `ice-config` /
  `ice-subscription`: callers pass `HostPlatform` (`crates/ice-types`).
  `ice-engine` maps the compile-time target and is used by the desktop
  shell. IPC commands and capture live in domain modules instead of two
  oversized files. Page tests look up copy through `t(key)` and
  `data-testid`, not zh literals or layout classNames.
- Desktop `build.rs` no longer emits cargo warnings on a successful
  sing-box / GeoIP copy, and skips the copy when the bundled file is
  already current (avoids a `tauri dev` rebuild loop).

### Fixed

- Windows TUN no longer prompts UAC on every enable: the `ice-box-tun`
  scheduled-task pin is stored via UTF-16 XML import (`schtasks /D` is a
  day-of-week flag, not a description).
- Windows TUN elevation no longer flashes a console window (UAC launches
  the GUI-subsystem `ice-tun-launcher` instead of `cmd.exe`).
- CI typecheck uses `npm run typecheck` (not `npx tsc`, which installs the
  stub `tsc` package) and installs `apps/website` dependencies so the Live
  Demo typecheck actually runs.
- Windows `cargo test -p ice-box --lib` embeds Common Controls v6 on the
  test harness so it no longer dies at process load
  (`STATUS_ENTRYPOINT_NOT_FOUND`); Tauri only put that manifest on the app
  exe.
- Windows NSIS actually bundles `libcronet.dll` next to `sing-box.exe`
  (NaiveProxy). Earlier notes claimed it shipped, but it was only copied
  into `resources/` and omitted from `bundle.resources`.
- Elevated sing-box no longer loads a user-writable config as-is: helper
  and Windows launcher sanitise JSON (`ice-config-guard`) and start from a
  root/admin-owned copy (`tun.config_rejected`). Subscription parse skips
  disallowed outbound types with warnings.
- Windows TUN copies launcher/core into `%ProgramData%\ice-box\bin`,
  imports task XML from that directory (not the user data dir), and
  verifies `Exec/Command` plus the baked `--data` config path.
- Helper token file is `0600` owned by the installing uid (uid check
  remains the primary control).
- `adopt_external` refuses pids that are not the bundled sing-box
  (`core.adopt_rejected`) and re-checks process start time before kill.
- `force_tun_gate_ready` is compiled only for tests / the `test-hooks`
  feature.
- macOS helper `SetDns` validates the service name and DNS server
  literals in the daemon before running `networksetup`
  (`tun.invalid_argument`).
- macOS TUN verify fails closed when the DNS probe errors instead of
  treating unknown DNS as consistent.
- `CoreController::stop` is idempotent while `Stopping` (spec: repeated
  stops succeed).
- Tray quit classifies a poisoned lock by `app.lock_poisoned` instead of
  a message substring.
- TUN network CIDR helper rejects IPv4 prefix > 32 and IPv6 prefix > 128
  instead of underflowing the mask.
- Core health after reload requires Clash API `GET /version` as well as TCP
  (CORE-2). Adopted pid liveness treats zombies as exited and Windows
  `ACCESS_DENIED` as alive (CORE-3/CORE-4). The traffic supervisor recovers
  from a panicking stream (CORE-5). Clash API errors are `core.api_failed`
  (CORE-8).
- App and core logs rotate at 20 MiB (keep 5 / 3). Generated core `log.level`
  defaults to `warn`, with a Settings toggle for `info`. The Logs page can
  truncate files in place (CORE-7).
- Subscription profile commit renames the previous dir aside instead of
  deleting it; a leftover `.old-*` is restored on load (SUB-1). Node/group/
  rule caps truncate with a warning instead of hard-failing (SUB-2). Fetch
  workers survive a poisoned queue and a panicking job (SUB-4). Missing
  subscription / IO errors use `sub.not_found` / `sub.io` (SUB-5).
- `validate_config` checks non-empty arrays, unique tags, and outbound
  references (CFG-1). Corrupt `settings.json` is quarantined and replaced
  with defaults plus a `settings.reset` banner (CFG-2). Runtime config
  restore from `.bak` is atomic (CFG-3).
- Corrupt `proxy-backup.json` is `Unknown` and takes the live-probe restore
  path instead of reading as "not applied" (PROXY-1). TUN recovery that
  cannot persist the journal returns `tun.recovery_required` (TUN-2).
- Helper token compare hashes both sides with SHA-256 and uses
  `subtle::ConstantTimeEq`, so a length mismatch cannot take an early return
  (SEC-7).
- Status polling reads an immutable core snapshot and never waits on
  start/stop; the shell emits `core://status-changed` (ORCH-1). `save_settings`
  merges a `SettingsPatch` so Home and Settings cannot clobber each other
  (ORCH-3).
- The UI uses one visibility-aware status poller (10 s fallback, pauses when
  hidden) plus core/traffic events (PERF-2 / FE-1). The traffic chart fetches
  only new samples (`get_traffic_since`) (PERF-3).
- IPC error codes live in `ice_config::ErrorCode` (including `tun.*` /
  `update.*`); the desktop types them from `errorCodes.ts` (ARCH-3).
- Windows TUN pin helpers live in `ice-tun-pin`; `ice-tun-sys` no longer
  depends on the launcher binary crate (ARCH-2). Elevated start liveness
  is shared across child and pid-file coordinators (ARCH-5).
- Subscription profile cache returns `Arc` on hit instead of cloning the
  profile body (SUB-6). macOS live proxy checks probe only the primary
  service (PROXY-2). Windows orphan-core reclaim enumerates processes the
  same way Unix already did (CORE-6).
- GeoIP rule-set presence is cached by directory mtime; rule fingerprints
  are SHA-256 of canonical JSON and still match older `rules.json` keys
  (PERF-1).
- Desktop drops unused `@tanstack/react-virtual`; `tailwindcss` is a
  devDependency. Website `api` is typed with `satisfies typeof tauri.api`
  (FE-4 / FE-7). Toolchain is pinned in `rust-toolchain.toml`; CI workflows
  share concurrency and action majors; GitHub Releases attach the root
  `LICENSE` (CI-3 / CI-4 / CI-7). `scripts/fetch-geoip.sh` pins a git ref
  and verifies SHA-256 (CI-8). Weekly `audit.yml` runs `cargo deny` and
  `npm audit --audit-level=high` (CI-2). GeoIP rule-sets are sourced only
  from `third_party/sing-geoip` (CI-6). Subscription TLS uses the OS trust
  store and a cached rustls config; gzip bodies are decoded (SUB-3). Settings
  is split into `settings/*` cards and follows `RuntimeStore` TUN transitions
  (FE-2). Known IPC errors are localized; outbound labels and core status go
  through `t()` (FE-5).

## [0.1.6] - 2026-09-06

### Fixed

- The GitHub Release pipeline now finds nested macOS updater archives when
  synthesizing `latest.json`. Installed 0.1.5 clients can install this build
  from Settings / the sidebar upgrade arrow.

## [0.1.5] - 2026-09-06

### Added

- In-app auto-update via the Tauri updater (minisign integrity). After this
  release is installed once, later versions can be downloaded from Settings.
  Background checks default on (`check_app_updates`); a green arrow next to the
  sidebar version opens the App Updates card, where Install Update appears
  beside Check for Updates. 0.1.4 and earlier cannot self-update into this
  release.

### Fixed

- Settings no longer reports a missing GitHub `latest.json` (unpublished
  updater catalog) as a GitHub connectivity / proxy failure.

### Changed

- GitHub Releases now publish macOS `.app.tar.gz` + `.sig`, the NSIS `.exe.sig`,
  and `latest.json` so installed 0.1.5+ clients can find the next build.

## [0.1.4] - 2026-09-06

### Added

- Restore the proxy service on launch when it was on at the last quit. A
  stopped service stays stopped. Capture restore waits until the window's
  first UI frame so system proxy / TUN do not contend with first paint;
  the core still starts in the background immediately.

### Changed

- Faster app launch: reuse a parsed subscription profile across config
  generation and the UI, overlap geoip copy with startup, skip duplicate
  Home status/settings fetches, and paint core status before nodes return.
  Orphan-core reclaim, system-proxy crash recovery, and TUN journal
  recovery run on the auto-start worker instead of the setup thread, so
  the window can appear while that work is still in flight.

## [0.1.3] - 2026-09-04

### Added

- Windows TUN as a production capture path. One UAC creates the per-user
  scheduled task `ice-box-tun`; afterwards start/stop need no extra prompt.
  Capture is IPv4 TCP only (DoT/DoH DNS, no fake-ip, UDP 443 rejected so
  browsers fall back from HTTP/3). UDP/QUIC and IPv6 still require system
  proxy. See `docs/tun.md`.
- Chinese / English UI with a system-locale default and a Settings language
  selector (the tray menu follows).
- Per-subscription auto-update (1 / 3 / 6 / 12 / 24 h) with a background
  refresh watchdog that also runs shortly after launch.
- macOS TUN `dns_hijack` points the primary network service at public
  resolvers so port-53 queries enter the TUN instead of a LAN resolver.

### Changed

- The TUN switch only chooses the next proxy-service start. Toggling it does
  not start, stop, or hot-swap the live capture.
- Subscriptions and Rules UI: simpler import form, search in the rules
  header, no separate apply-config button.
- Documentation: README rewritten; living docs are `docs/architecture.md`,
  `docs/tun.md`, and `docs/release-process.md`. Completed plans and spike
  diaries were removed.

### Fixed

- Reclaim orphan cores and release mixed/clash ports across TUN hand-offs
  and wake, including the elevated Windows core (`PidProcess` liveness).
- Windows TUN: locale-proof `netsh` / `route print` parsers, adapter-index
  identity, graceful WFP teardown, silent console windows, launcher `/TR`
  length under the schtasks limit.
- Traffic stream treats a reset read as EOF; the home-page tooltip shows
  units.
- Subscription apply keeps the latest metadata and refreshes the timestamp
  on HTTP 304.

## [0.1.2] - 2026-08-31

### Added

- Proxy URI list ("share link") subscription support: subscriptions whose body
  is a base64-wrapped or plain list of `vless://`, `vmess://`, `trojan://`,
  `ss://`, `hysteria://`, `hysteria2://`, `tuic://`, `socks://`, `http(s)://`,
  `wireguard://` links now import as nodes (v2rayN / v2rayNG / ClashMeta /
  Hiddify / sing-box converter formats, incl. reality, vision flow, ws/grpc/
  http transports, SIP002 + legacy ss, v2rayN base64 vmess). Every link is
  preserved as-is, including provider metadata links (e.g. `剩余流量：...`);
  `ssr://` and unsupported flows/transports are skipped with per-line warnings
  because sing-box cannot run them.

- URI list imports route through the injected `proxy` selector (flat profiles),
  so node selection actually takes effect; reality outbounds always carry a uTLS
  fingerprint (`uTLS is required by reality client`); hysteria2 links with
  `pinSHA256` degrade to `insecure` because sing-box has no cert-pinning support
  (providers ship pins precisely because their certificates are not
  standards-compliant and would otherwise fail TLS verification).

- Built-in split routing for rule-less subscriptions (share-link URI lists, or
  any Clash / sing-box body without rules): private IPs, Chinese IPs
  (`geoip-cn`) and a curated list of ~170 common Chinese domain suffixes go
  direct, everything else follows the selected node. A matching DNS block
  routes Chinese domains to 223.5.5.5 and everything else to a remote DoH
  through the proxy (anti-pollution). The defaults are attached at profile
  load time (not baked into the cache), so the Rules page shows them and they
  stay individually toggleable via rule overrides. A new Settings toggle
  「为无规则的订阅附加默认分流规则」(on by default) turns the whole feature
  off. The domain tier is designed to be swappable for a bundled `geosite-cn`
  rule-set later.

- In-app privileged helper installation for TUN mode (macOS): the app prompts
  the system authorization dialog (`AuthorizationServices`) and installs or
  removes the `ice-helper` launchd daemon itself, replacing the manual
  `sudo` script as the primary flow. The install logic now lives once in
  `crates/ice-helper/src/install.rs` (token, plist, pinned SHA-256, launchctl)
  and is shared with `scripts/install-helper-macos.sh` /
  `uninstall-helper-macos.sh`. New IPC commands `install_helper` /
  `uninstall_helper`; Home and Settings surface「安装辅助组件」/「卸载辅助组件」
  actions.

- Windows TUN backend (T2 shape, host-free): `WindowsTunBackend` in
  `ice-tun-sys` with read-only `netsh` / `route print` host probes (host-free
  parsing tests on all CI hosts), interface-index identity, observed-route
  ownership, journaled apply/verify/restore/recover, and the dev elevated
  core runner (`WindowsElevatedCoreCoordinator`, `taskkill /T /F` stop).
  Wired by `create_backend` behind the explicit `ICE_BOX_TUN_WINDOWS_DEV`
  opt-in only — production Windows stays fail-closed until the
  `windows_tun_ready` T0 spike passes on a real host
  (`scripts/run-acceptance-windows-tun.sh`, G9.14 live gate;
  `docs/tun.md`).

- `scripts/fetch-singbox.sh` / `prepare-singbox-resource.sh` / `.ps1` now
  also ship `libcronet.dll` next to `sing-box.exe` on Windows (NaiveProxy
  outbound companion from the pinned archive; the wintun driver is embedded
  in the binary).

### Changed

- The macOS release is permanently unsigned (documented product decision):
  code signing, notarization, and SMAppService are not part of the product;
  all signing-related content was removed from the documentation. Gatekeeper
  warnings are expected on published artifacts.

## [0.1.1] - 2026-08-28

First public release. Tauri 2 + React desktop client for macOS (Apple Silicon) and
Windows with a bundled sing-box core (1.13.19).

### Added

- Desktop app shell: liquid-glass layout, sidebar navigation, native window
  chrome, min window size 720x500
- Subscriptions: import from URL, update, one active subscription at a time
  (sing-box first, Clash compatible), direct-only mode without a subscription
- System proxy integration (macOS `networksetup`, Windows WinInet, per-user, no
  elevation) with faithful restore on exit
- Modes: one-click Rule / Global / Direct switching, live via the Clash API
- Nodes: outbound switching, latency tests, collapsible strategy groups
  (select / url-test / fallback / load-balance)
- Rules: server-side search / filter / pagination, disable / enable with
  persisted fingerprint, custom rules
- Traffic: real-time upload / download chart with 60s history, stale-data
  detection after consecutive sample failures
- Logs view, settings (auto start, auto system proxy, allow LAN, ports), error
  shape with stable error codes
- Core lifecycle: hot reload (SIGHUP) with restart fallback, health checks,
  rollback on failure
- GEOIP/GEOSITE expansion to bundled sing-geoip rule-sets
- macOS `.dmg` and Windows NSIS installers

### Changed

- Redesigned home page: large proxy power toggle, mode controls that avoid
  false error flashes during start / reload
- Traffic chart sampling hardened: paused during busy work, in-flight reset on
  re-poll

### Security

- Subscription fetch hardened: SSRF validation (loopback / private / fake-IP
  blocked), pinned DNS resolution, TLS redirect downgrade refused, URL
  redaction in logs

### Documentation

- MIT license, `NOTICE` with third-party notices (bundled sing-box GPL-3.0-or-later
  and Twemoji flag fonts)