# Changelog

All notable changes to this project are documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

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
  `docs/design-notes/tun-windows-t0.md`).

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