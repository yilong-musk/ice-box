# Changelog

All notable changes to this project are documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- In-app privileged helper installation for TUN mode (macOS): the app prompts
  the system authorization dialog (`AuthorizationServices`) and installs or
  removes the `ice-helper` launchd daemon itself, replacing the manual
  `sudo` script as the primary flow. The install logic now lives once in
  `crates/ice-helper/src/install.rs` (token, plist, pinned SHA-256, launchctl)
  and is shared with `scripts/install-helper-macos.sh` /
  `uninstall-helper-macos.sh`. New IPC commands `install_helper` /
  `uninstall_helper`; Home and Settings surface「安装辅助组件」/「卸载辅助组件」
  actions.

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