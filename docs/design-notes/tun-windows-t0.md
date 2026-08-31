# Windows T0 spike: WinTUN feasibility and backend lock

Status: **blocked — host spike run (2026-08-31, Windows 11 host, elevated)**.
Adapter creation, routes, DNS, and cleanup all work, but the pinned
sing-box 1.13.19 cannot complete outbound traffic on Windows: every
sing-box-originated packet (direct outbound, proxy upstream, DNS queries)
is captured by its own TUN and loops (upstream issue
[SagerNet/sing-box#4455](https://github.com/SagerNet/sing-box/issues/4455),
reported against exactly 1.13.19). **1.14.0 stable (released 2026-08-31)
was tested live and still has the outbound black hole** (HTTP 000 for
direct and proxy; host DNS OK only with `strict_route: false`), so
upgrading the pin does not unlock Windows TUN. `windows_tun_ready` stays
pending until the upstream issue is truly fixed.

Related: `docs/tun-mode-plan.md` (§3 feasibility spike, §5 slices T0–T5),
`docs/design-notes/tun-t0-spike.md` (macOS analogue, green),
`docs/architecture.md` §24.

## 1. Host-spike results (2026-08-31, elevated, real Windows 11)

Host: `win-espf39otjbu\admin`, elevated, WSL2 NAT environment (physical NIC
index 4, `10.28.10.0/24`, gateway `10.28.10.1`; plus Hyper-V vEthernet
interfaces 18/37). Every run left **zero residue** (verified: no adapter, no
routes via `10.0.0.1/10.0.0.2`, default route intact, DNS resolution OK,
ping 7 ms 0% loss).

| # | Fact | Result |
|---|------|--------|
| 1 | wintun driver is embedded in the binary | Confirmed: the release zip ships only `sing-box.exe` + `libcronet.dll`; no side-by-side `wintun.dll` is needed |
| 2 | Elevation model | Unelevated start fails cleanly: `FATAL ... configure tun interface: Access is denied.` — no adapter, no residue. Elevated start works. Admin is required for adapter creation. |
| 3 | Adapter creation (locked config) | `Wintun` adapter created, interface **index 42**, MTU 9000, state connected; IPv4 `10.0.0.1/30`; IPv6 `fdfe:dcba:9876::1` (manual, no prefix reported by netsh) |
| 4 | Route shape (locked config) | **The sub-range trick is used on Windows too**, split around `route_exclude_address`: v4 owned set via `10.0.0.1` = `0.0.0.0/5, 8.0.0.0/7, 11.0.0.0/8, 12.0.0.0/6, 16.0.0.0/4, 32.0.0.0/3, 64.0.0.0/3, 96.0.0.0/4, 112.0.0.0/5, 120.0.0.0/6, 124.0.0.0/7, 126.0.0.0/8, 128.0.0.0/3, 160.0.0.0/5, 168.0.0.0/8, 169.0.0.0/9 … 169.255.0.0/16 (169.254/16 excluded), 170.0.0.0/7, 172.0.0.0/12, 172.32.0.0/11, 172.64.0.0/10, 172.128.0.0/9, 173.0.0.0/8, 174.0.0.0/7, 176.0.0.0/4, 192.0.0.0/9, 192.128.0.0/11, 192.160.0.0/13, 192.169.0.0/16, 192.170.0.0/15, 192.172.0.0/14, 192.176.0.0/12, 192.192.0.0/10, 193.0.0.0/8, 194.0.0.0/7, 196.0.0.0/6, 200.0.0.0/5, 208.0.0.0/4, 240.0.0.0/4` (gateway `10.0.0.2`). v6 owned set via index 42 = `::/1, 8000::/2, c000::/3, e000::/4, f000::/5, f800::/6, fe00::/9, fec0::/10` (gateway `fdfe:dcba:9876::2`; `fc00::/7`, `fe80::/10`, `ff00::/8` excluded). The default route `0.0.0.0/0 → 10.28.10.1` stays present but the sub-ranges win. |
| 5 | DNS ownership | **Windows differs from macOS**: sing-box sets the adapter's DNS servers to the derived TUN peers (`10.0.0.2` IPv4 + `fdfe:dcba:9876::2` IPv6) on the Wintun interface. The backend must claim DNS ownership (journal `dns_before`/`dns_after` + compare-before-restore) on Windows. |
| 6 | Graceful stop | `GenerateConsoleCtrlEvent(CTRL_C_EVENT)` → sing-box exits, adapter and routes removed. Clean. |
| 7 | Hard kill (`taskkill /T /F`) | Adapter removed, zero route/DNS residue (on this host; the 1.13 log-storm run also left zero residue). |
| 8 | netsh probe syntax | `netsh interface ipv4 show addresses` / `show dnsservers` accept `name=` only; `netsh interface ipv6 show addresses` accepts `interface=` only. The Windows backend's v6 probe must use `interface=`. |
| 9 | 1.13.19 outbound loop (BLOCKER) | With a live core, every sing-box-originated outbound is captured by its own TUN: DNS via the physical NIC resolver (`dial udp 61.130.254.34:53: i/o timeout`), direct TCP (`dial tcp 120.77.242.231:1688: i/o timeout`), mixed-proxy requests (HTTP 000), UDP direct (`listen udp4 :0: An invalid argument was supplied`). 102,986 log lines in ~90 s; system DNS breaks while TUN is active; high CPU. Matches upstream #4455 (same 1.13.19). |
| 10 | Loop workaround (#4455) | `{ "ip_cidr": [tun CIDRs], "action": "reject", "method": "drop" }` after hijack-dns and before `ip_is_private` stops the self-ignited loop (system DNS → TUN peer), but **does not fix user-traffic outbound**: mixed-proxy still HTTP 000, host DNS still fails, CPU still high. |
| 11 | 1.14.0-rc.1 | Same locked config: adapter + DNS OK; unbounded loop **fixed** (log 3 → 3 lines) but outbound still broken (mixed-proxy HTTP 502, host DNS timeout, ~275% CPU during the test). Not a drop-in fix for this release. |
| 12 | 1.14.0 stable (2026-08-31) | TUN adapter renamed to **`tun0` / "sing-tun Tunnel"** (not "Wintun"). Adapter + routes + no unbounded loop all OK. With `strict_route: true` the new WFP filters inject malformed packets into the DNS path (`unpack request: bad question name: dns: buffer size too small`, `bad question size: 0`), breaking host DNS. With `strict_route: false` those errors vanish and host DNS works — **but outbound is still a black hole**: direct curl and proxy curl both HTTP 000 (10 s timeout). Outbound black hole is independent of WFP/strict_route and of dns_mode. `auto_detect_interface` was removed from the TUN inbound in 1.14 (route-level field only); the default-outbound field is `final` again (1.13's `default_outbound` was removed). |

## 2. Root cause of the blocker

On Windows, `route.auto_detect_interface` binds sing-box's outbound sockets
to the default (physical) interface. Windows routing then has no
"bound-socket prefers its interface" guarantee (weak-host model off by
default): a packet whose route table entry points at a *more specific* route
— the TUN sub-ranges — is either dropped (`invalid argument` for UDP) or
delivered into the TUN. Without the binding, the same sub-ranges capture the
packet too. Either way sing-box cannot send a packet to the physical
network while its own TUN sub-range routes cover public addresses. macOS
does not exhibit this (its bound-interface semantics differ), which is why
the macOS gate passed.

This is the platform-gate failure the plan predicted (§3.1): "If `1.13.19`
cannot provide the required behavior on one platform, block only that
platform's TUN slices and decide whether to upgrade the pinned core or add
a platform adapter before enabling TUN there."

## 3. Decision options (recorded, not deferred silently)

1. **Upgrade the pinned core** — **tested and rejected**: 1.14.0 stable
   (the only newer release) still has the Windows TUN outbound black hole
   (item 12). Upgrading re-pins the schema for **all platforms** and
   requires re-running the macOS live gates (G9.12/G9.13), so it is not
   worth it until a release actually fixes the loop.
2. **Wait for the upstream fix** and keep `windows_tun_ready` pending;
   Windows TUN stays fail-closed (`tun_available=false`). Recommended.
3. **Platform adapter**: bundle a different Windows core (mihomo) for TUN
   only — rejected by the locked "sing-box first, no mihomo fork" decision
   (windows-plan §1); would need an explicit plan amendment.

## 4. What landed behind the gate (unchanged)

**UI**: the desktop app hides all TUN controls on Windows while the gate is
pending (`status.tun_ui_hidden=true`, derived from the backend capability in
`capture.rs` rather than a bare `cfg!` — so the controls reappear
automatically when the gate turns green, and the `ICE_BOX_TUN_WINDOWS_DEV`
opt-in keeps them visible; the Settings TUN card, the Home TUN toggle, the
helper-install dialog, and TUN-derived power control state are not rendered).
The TUN settings shape and save path stay intact so nothing breaks when the
gate turns green.

`crates/ice-tun-sys/src/windows.rs` stays fail-closed (`UnsupportedTunBackend`
in production; the real backend behind `ICE_BOX_TUN_WINDOWS_DEV`). It already
matches the confirmed host facts: interface-index identity, route matching by
IPv4 interface-IP / IPv6 index, and the sub-range route shape (the backend
should adopt the observed v4/v6 sub-range sets from §1 item 4 as locked
constants once the gate turns green). The v6 netsh probe already uses
`interface=` (item 8) and treats only a confirmed missing interface as
"gone" — any other probe failure fails closed instead of being misread as a
verified missing interface. Two confirmed fixes are still pending the gate:
DNS ownership must be claimed on Windows (item 5), and the interface
discovery must also match the 1.14+ adapter naming `tun0` / "sing-tun
Tunnel" in addition to "Wintun" (item 12).

## 5. Safe-testing protocol (this spike)

Every run: warn-level core logs, short curl timeouts, forced
`taskkill /T /F` cleanup, and a post-run residue + network-health check
(no adapter, no `10.0.0.x` routes, default route intact, DNS resolves).
All runs passed cleanup; no user-visible breakage occurred.