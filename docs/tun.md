# TUN capture

This document is the desktop TUN path on macOS, Windows, and Linux, with the
bundled sing-box **1.14.4**. Android captures traffic through the system VPN
service; see [architecture.md](architecture.md#clients). Desktop capture
behavior, platform constraints, and recovery rules follow.

| Platform | Coverage | Setup |
|----------|----------|-------|
| macOS | IPv4 + IPv6, TCP and UDP | System authorization installs the privileged helper |
| Windows | IPv4 TCP only; no UDP/QUIC, IPv6, or fake-ip | One UAC prompt creates a scheduled task; the user must already be an Administrator |
| Linux | IPv4 + IPv6, TCP and UDP | Polkit (`pkexec`) installs the systemd `ice-helper` service |

## Product model

- TUN defaults off. While the proxy service is running, the Settings/Home
  switch switches the active capture backend immediately (system proxy ↔ TUN);
  the save path commits only after the new backend is healthy and rolls back on
  failure. With the service stopped it only records the next-start choice.
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
| Disable | Mark restoring, release owned resources, verify cleanup, mark clean; stopping TUN leaves the core stopped until the next start, a live TUN → system switch restarts the diagnostic core and applies the system proxy, and system-proxy disable restores the OS proxy and keeps the core running |
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
sniff fields. Sniffing precedes domain-based policy rules. The desktop TUN
inbound pins `dns_mode` explicitly: `native` on Windows and `disabled` on
macOS and Linux. sing-box 1.14 defaults to `hijack` when `auto_route` is enabled; on
Windows that installs WFP filters blocking port 53 outside the TUN, which
would break LAN DNS because those routes are excluded from capture.

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
- The TUN inbound sets `dns_mode: disabled`: system DNS belongs to this backend,
  not the core (sing-tun has no Darwin native-DNS path, and a future one must
  not silently take ownership). DNS is captured by the explicit `hijack-dns`
  route rules.
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

The helper carries no Developer ID signature: it keeps its toolchain (linker)
ad-hoc signature, ships inside the app bundle (`Contents/Resources`), and is
covered by the bundle-level ad-hoc seal. Manual install/uninstall scripts are
under `scripts/`; the developer sudo runner is test-only. The clean-machine
install gate is waived; helper install/enable/disable/uninstall remains covered
by the manual acceptance script in [testing.md](testing.md#live-tests).

### Windows

- The per-user `ice-box-tun` scheduled task runs protected launcher/core copies
  under `%ProgramFiles%\ice-box`. Its identity and binary hashes are pinned.
  Config is sanitized into an admin-owned run directory; each `rule_set` file
  is decoded and re-encoded as canonical source JSON before it is staged, and
  staged files get explicit SYSTEM/Administrators file DACLs (directory `(OI)(CI)`
  grants are not applied to files via `/T`). Standard-user over-the-shoulder
  UAC is refused. If Task Scheduler rejects the unsigned
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
- The TUN inbound sets `dns_mode: native`, preserving the adapter-DNS/TUN-peer
  behaviour without sing-box 1.14's WFP port-53 block (which would also block
  LAN resolvers on excluded routes).
- Use TCP DNS (DoT/DoH), anchored by an IP-addressed server, with `ipv4_only`.
  Local DNS loops through the adapter; UDP DNS can be captured by its own TUN;
  fake-ip addresses are unroutable. Reject UDP 443 with ICMP so browsers fall
  back to TCP instead of waiting on QUIC.
- IPv6 stays disabled for Windows TUN even when Settings → IPv6 is on: the
  pinned core's IPv6 DNS path is broken (#4178) and the hijack rule is
  IPv4-only. The IPv6 switches still apply to system-proxy mode; macOS, Linux,
  and Android TUN follow them.
- Re-normalize cached profiles so older DNS configurations adopt these rules.
  System proxy is the alternative capture path for applications needing traffic
  that Windows TUN cannot carry.

### Linux

- Both IPv4 and IPv6 addresses are supported (subject to Settings → IPv6). The
  TUN interface name must fit `IFNAMSIZ - 1` (15 bytes, e.g., `ice0`), and
  interface identity is locked by both the interface name and its Linux
  `ifindex` so recovery never tears down an unrelated interface.
- sing-box owns the kernel `tun` device and policy routes (`auto_route: true`,
  `strict_route: true`), while the backend journals and verifies them. The TUN
  inbound sets `dns_mode: disabled` so system DNS remains under `LinuxTunBackend`
  and `ice-helper`, with DNS captured by the explicit `hijack-dns` route rules.
- Before starting the elevated core, wait until `ip -4 route get 1.1.1.1`
  resolves to a physical NIC (including `ppp*` PPPoE uplinks, but never a
  leftover TUN/loopback device) and pin that name as `route.default_interface`
  with `auto_detect_interface: false`. `process_name` bypass rules exclude
  `ice-box` and `sing-box` so direct origin probes, subscription fetches, and
  update checks avoid routing loops.
- With `dns_hijack`:
  - When `systemd-resolved` is active **and** `/etc/resolv.conf` routes through
    its stub listener (`stub-resolv.conf` symlink or `127.0.0.53`/`127.0.0.54`),
    `ice-helper` configures per-link DNS on the TUN interface via `resolvectl`
    (`dns`, `domain ~.`, `default-route true`, `flush-caches`) and reverts it
    best-effort on disable or recovery.
  - Otherwise, if `/etc/resolv.conf` is a regular file (not a symlink),
    `ice-helper` backs up its original raw bytes to a root-only file
    (`/usr/local/libexec/ice-box/run/resolv.conf.ice-box-backup`, `0600`)
    before atomically writing the managed resolver file, preserving `search`,
    `domain`, `options`, link-local `%scope`, and arbitrary nameserver counts.
    Restore compares current servers before writing the backed-up bytes back
    verbatim (and also runs during helper uninstall if the file still carries
    the managed marker), preserving external edits made while TUN was active.
    Symlinked `/etc/resolv.conf` files without `systemd-resolved` stub routing
    are left untouched rather than clobbering another network manager's target.

### Linux elevation

The app installs and removes `ice-helper` through Polkit (`pkexec`). The
hardened systemd service (`/etc/systemd/system/ice-box-helper.service`,
`Restart=on-failure`) loads the per-installation auth token from a root-only
`EnvironmentFile` (`/etc/ice-box/helper.env`, `0600`), listens on
`/run/ice-box-helper.sock`, authenticates the peer UID (`SO_PEERCRED`) and
token, pins the installed binaries under `/usr/local/libexec/ice-box/`
(`ice-helper`, `sing-box`, `run/`, `resources/`), and writes logs to
`/var/log/ice-box-{core,helper}.log`.

Because an AppImage unpacks into a user-private FUSE mount (`/tmp/.mount_*`)
that root cannot traverse or rely on across launches, in-app Polkit
install/uninstall stages `ice-helper`, `sing-box`, and `geoip/*.srs` into a
transient user-owned `0700` temp directory before invoking `pkexec`, and the
root installer copies bundled `geoip/*.srs` rule-sets into
`/usr/local/libexec/ice-box/resources/geoip` and sanitizes user configs into a
root-owned run directory before invoking `sing-box`. Manual install/uninstall
scripts (`scripts/install-helper-linux.sh`, `scripts/uninstall-helper-linux.sh`)
are provided alongside the in-app Polkit flow; live acceptance scripts are
documented in [testing.md](testing.md#live-tests).

## Validation and source

Long-running stability, Wi-Fi changes, and macOS MTU 9000 over real VPN/QUIC
paths remain unverified. Windows G9.14 Cargo acceptance on an MSVC host and
live Linux desktop acceptance across GNOME/KDE + systemd hosts remain manual
validation items. Manual commands and prerequisites live in
[testing.md](testing.md#live-tests).

Exact structures and platform mechanics are maintained with their source:

- [Capture controller](../apps/desktop/src-tauri/src/capture/mod.rs): runtime status and transitions.
- [Journal](../crates/ice-tun-journal/src/lib.rs): persisted ownership and recovery records.
- [Helper protocol](../crates/ice-tun-helper-proto/src/protocol.rs) and [errors](../crates/ice-types/src/error.rs): IPC contracts.
- [macOS and Linux helper](../crates/ice-helper/src/lib.rs) and [Windows launcher](../crates/ice-tun-launcher/src/main.rs): privileged execution.
