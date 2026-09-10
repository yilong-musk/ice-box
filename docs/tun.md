# TUN capture

TUN is supported on macOS and Windows with the bundled sing-box **1.13.19**.
This document owns capture behavior, platform constraints, and recovery rules.

| Platform | Coverage | Setup |
|----------|----------|-------|
| macOS | IPv4 + IPv6, TCP and UDP | System authorization installs the privileged helper |
| Windows | IPv4 TCP only; no UDP/QUIC, IPv6, or fake-ip | One UAC prompt creates a scheduled task; the user must already be an Administrator |

## Product model

- TUN defaults off. The Settings/Home switch only selects the backend for the
  next service start; changing it never switches active capture.
- Home starts/stops the proxy service. Stop releases the active backend even
  if the saved choice has changed. System proxy and TUN are mutually exclusive;
  capture selection is independent of Rule / Global / Direct routing policy.
- The runtime controller owns active capture. Saved `configured_tun` can differ
  from `traffic_capture`; the UI reads runtime status rather than inferring it
  from settings.
- Automatic core start uses a Mixed-only diagnostic config. TUN configs retain
  Mixed for diagnostics. Restoring the saved service choice is a separate
  action after startup recovery.

## Lifecycle and recovery

```text
Disabled -> Preparing -> Enabled -> Stopping -> Disabled
                \-> PermissionRequired / Error / RecoveryRequired
```

Core readiness is required before capture is enabled. Start, stop, reload, and
recovery share the application's orchestration lock.

| Operation | Required ordering |
|-----------|-------------------|
| Enable | Journal intent, build/validate config, start/reload core, verify Clash API and TUN health, mark applied |
| Disable | Mark restoring, release owned resources, verify cleanup, mark clean; the core may continue in diagnostic mode |
| Topology change | Disable, validate, re-enable; commit settings after health checks, otherwise roll back |
| Policy change | Reload and verify health; a restart that removes resources requires re-application |
| Core failure or quit | Release capture and restore diagnostic config as appropriate; quit then stops the core |

`tun-state.json` is an atomic mutation journal, independent of the system-proxy
backup. Record intent before OS changes and progress after each mutation.
Recovery verifies installation ownership and resumes idempotently. Remove only
verified owned resources. Restore DNS only if it still matches the app's last
applied value; preserve external changes.

Uncertain cleanup produces `recovery_required`: both backends remain disabled
until recovery succeeds. The UI offers `recover_tun`; the watchdog and next
startup can also retry cleanup. Recovery never enables capture. Permission
failures may offer system-proxy fallback only when no TUN resources remain.

## Platform constraints

Generated configurations must match the pinned core: use `address`, not
`inet4_address` / `inet6_address`, and route-level `action: sniff`, not inbound
sniff fields. Sniffing precedes domain-based policy rules.

Reserved routing protects loopback, local/link-local/multicast ranges, control
endpoints, and resolver traffic. The TUN CIDR has platform-specific handling;
Windows rejects peer traffic after DNS hijack. `allow_lan` does not expand the
bypass policy. Rule ordering is significant; see the
[configuration builder](../crates/ice-config/src/build.rs).

### macOS

- Both IPv4 and IPv6 addresses are required: an IPv4-only TUN leaves IPv6
  outside capture. Interface identity is the selected `utun<N>` and its index.
- sing-box owns the native adapter and routes; the backend journals and
  verifies them. DNS changes use the helper. Even after a process exit removes
  the utun, recovery must verify cleanup.
- With `dns_hijack`, set the primary service's DNS to public resolvers so
  queries enter TUN instead of staying on a LAN resolver. Journal the previous
  DNS and compare before restoring it. Core process bypass precedes DNS hijack
  to prevent the core's own DNS requests from looping.
- Before starting the elevated core, wait until `0.0.0.0` resolves to a
  physical NIC (not a leftover `utun`) and pin that name as
  `route.default_interface` with `auto_detect_interface: false`. Launch restore
  otherwise can look healthy while Direct / proxy dials follow a dying tunnel.

### macOS elevation

The app installs/removes `ice-helper` through system authorization. The launchd
helper runs the bundled core as root, authenticates the peer UID and installation
token, pins the core binary, and sanitizes config into a root-owned copy before
execution. User config is opened without following a final-component symlink
and is bounded to 8 MiB. The token file is recreated (unlink, exclusive create, `fchown` on
the fd) so a pre-planted symlink in the user data dir cannot redirect the
privileged write. Elevated DNS operations validate arguments before touching the OS.
Missing or stale helpers require setup; they never trigger silent elevation.

The helper is intentionally unsigned. Manual install/uninstall scripts are
under `scripts/`; the developer sudo runner is test-only. The clean-machine
install gate is waived; helper install/enable/disable/uninstall remains covered
by the manual acceptance script in [testing.md](testing.md#live-tests).

### Windows

- The per-user `ice-box-tun` scheduled task runs protected launcher/core copies
  under `%ProgramFiles%\ice-box`. Its identity and binary hashes are pinned.
  Config is sanitized into an admin-owned run directory; standard-user
  over-the-shoulder UAC is refused. If Task Scheduler rejects the unsigned
  launcher (`0x80004005`), the task runs Microsoft-signed `wscript.exe` with
  an admin-owned `ice-tun-run.vbs`. PowerShell and `cmd.exe` are not used.
- Start/stop after setup needs no new UAC prompt. Stop must be graceful: stranded
  `strict_route` WFP filters can block host TCP. Forced termination is a last
  resort. WinTUN is embedded in the bundled core.
- Before starting the elevated core, wait until `0.0.0.0` resolves to a
  physical NIC (not leftover Wintun / TAP / loopback) and pin that adapter
  name as `route.default_interface` with `auto_detect_interface: false`.
  Launch restore otherwise can look healthy while Direct / proxy dials follow
  a dying tunnel.
- IPv4 port-53 hijack must come first, followed by process bypass, sniffing,
  and TUN-peer rejection. Do not add post-sniff `protocol: dns` hijack; it can
  misclassify unrelated UDP traffic.
- Use TCP DNS (DoT/DoH), anchored by an IP-addressed server, with `ipv4_only`.
  Local DNS loops through the adapter; UDP DNS can be captured by its own TUN;
  fake-ip addresses are unroutable. Reject UDP 443 with ICMP so browsers fall
  back to TCP instead of waiting on QUIC.
- Re-normalize cached profiles so older DNS configurations adopt these rules.
  System proxy is the alternative capture path for applications needing traffic
  that Windows TUN cannot carry.

## Validation and source

Long-running stability, Wi-Fi changes, and macOS MTU 9000 over real VPN/QUIC
paths remain unverified. Windows G9.14 Cargo acceptance on an MSVC host remains
an open validation item. Manual commands and prerequisites live in
[testing.md](testing.md#live-tests).

Exact structures and platform mechanics are maintained with their source:

- [Capture controller](../apps/desktop/src-tauri/src/capture/mod.rs): runtime status and transitions.
- [Journal](../crates/ice-tun-journal/src/lib.rs): persisted ownership and recovery records.
- [Helper protocol](../crates/ice-tun-helper-proto/src/protocol.rs) and [errors](../crates/ice-types/src/error.rs): IPC contracts.
- [macOS helper](../crates/ice-helper/src/lib.rs) and [Windows launcher](../crates/ice-tun-launcher/src/main.rs): privileged execution.
