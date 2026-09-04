# Windows T0 spike: WinTUN feasibility and backend lock

Status: **landed — `windows_tun_ready` flipped 2026-09-03** (host spike
re-verification). The V5–V10 progression found a Windows TUN shape that
works with the pinned 1.13.19 (and 1.14.0): `{"port": [53], "action":
"hijack-dns"}` as the first route rule, TCP-transport DNS servers
(DoT/DoH — UDP DNS upstreams are black-holed), `dns.strategy:
ipv4_only`, and the #4455 peer-reject rule. With that shape, system DNS
(`nslookup`), TUN-captured IPv4 TCP direct traffic, and the mixed proxy
all return HTTP 200 on both versions, with idle CPU. The gate flipped
behind the §1.2 shape in `ice-config` / `ice-subscription` (Windows-only
emission: no fakeip — V11 proved the 198.18.0.0/15 answers are
unreachable — no `local` server, UDP upstreams rewritten to DoT, and the
resolution anchor must be an IP-hosted TCP server — V12's domain-hosted
DoH final is a startup FATAL) and the backend (DNS ownership with
compare-before-restore, graceful-stop core runner — stranded WFP filters
black-hole host TCP). V13 re-ran the full production-like shape against a
**real subscription node** (trojan, domain host): TUN direct, mixed proxy
→ google/youtube through the node (200, 0.27–0.55 s), system DNS via the
TUN peer — all green. The cargo-based live acceptance (G9.14) remains to
be run on a host with an MSVC toolchain (the acceptance host lacks
`link.exe`). Two core bugs remain recorded but no longer block the
product: UDP outbound dials are captured by the core's own TUN
(workaround: TCP DNS), and the IPv6 path is broken (upstream #4178,
open; workaround: `ipv4_only`). UDP user traffic and long-running
stability are not yet verified — they are the next step, not a release
gate.

Related: `docs/tun-mode-plan.md` (§3 feasibility spike, §5 slices T0–T5),
`docs/design-notes/tun-t0-spike.md` (macOS analogue, green),
`docs/architecture.md` §24.

## 1. Host-spike results (2026-08-31, elevated, real Windows 11)

### 1.1 Re-verification on the real host (2026-09-03)

Driven from WSL via elevated PowerShell (UAC prompt). Binaries: the
installed `D:\Apps\ice-box\sing-box.exe` (1.13.19, revision `b5ebaa1`, the
exact pin) and the official 1.14.0 stable (revision `0b89958`). Every run
followed the safe-testing protocol in §5: warn-level logs, short curl
timeouts, `taskkill /T /F` cleanup, post-run residue + network-health
check — all clean (adapter removed, zero `10.0.0.x` routes, HTTP 200 /
0.06 s after).

Config under test: production-shaped `Tun` config (locked TUN inbound,
mixed inbound on a dedicated port, `route.auto_detect_interface`, DNS
block, `hijack-dns`, the #4455 peer-reject rule, process-name bypass).
Variants: `stack` gvisor/system; DNS with fakeip+local resolver or real
UDP resolvers (223.5.5.5) and sniff-before-hijack ordering;
`route_exclude_address` additions for exact destinations.

| # | Test (through the mixed proxy unless noted) | 1.13.19 | 1.14.0 |
|---|---------------------------------------------|---------|--------|
| 1 | Request to a **bare IP** (no DNS involved) | 200, 0.03 s | 200, 0.03 s |
| 2 | Request to a **domain**, http | 000, 7 s | 502, 4.5 s (502 = the proxy reporting resolution failure — corrected by V5–V6, §1.2) |
| 3 | Request to a **domain**, https | 000, 7 s | 000, 7 s |
| 4 | TUN-captured direct (system DNS) | 000, 7 s | 000, 7 s |
| 5 | `nslookup` (system resolver) | timeout | timeout |
| 6 | Core CPU over 4 s while failing | 4.7–6.1 s | 0.1–5.2 s (low once the DNS dial stopped spinning) |
| 7 | Adapter description | "sing-tun Tunnel" (not "Wintun") | "sing-tun Tunnel" |

Findings:

- **Exact-IP dials escape the TUN.** Row 1 succeeded with no route
  exclusion: the bound outbound socket reaches the physical NIC. This
  disproves the "every sing-box-originated packet is captured by its own
  TUN" reading in the 2026-08-31 §2.
- **System DNS is dead in every shape.** The adapter DNS is set to the TUN
  peers (`10.0.0.2` / `fdfe:dcba:9876::2`, confirmed on both versions).
  Queries to the peer are never hijacked — neither with `sniff` before
  `hijack-dns` nor with `hijack-dns` first — and fall into the peer-reject
  drop. `nslookup` always times out.
- **Domain dials fail after successful resolution** (rows 2–3) while the
  same IP dialed bare succeeds (row 1). The domain-carried dial path is
  not repaired by 1.14.0.
- DNS server option `detour: "direct"` is a runtime FATAL on both versions
  (`start dns/udp[dns-remote-1]: detour to an empty direct outbound makes
  no sense`); `sing-box check` passes but the core refuses to start.
- `stack=system` changes nothing (same failures as gvisor).
- Route-excluding the exact destination IP makes that destination work
  through the proxy (200, 0.03 s) while non-excluded destinations still
  fail — diagnostics only, not a viable product path.

### 1.2 Unlock progression (2026-09-03, V5–V10)

The V5–V10 runs walked from "every shape fails" to a working shape. All
runs kept the §5 safe-testing protocol; every variant left zero residue.
All tests through the mixed proxy and TUN-captured curls used short
timeouts; `--noproxy` was added from V7 on because the running ice-box
app had set the WinInet system proxy, which pollutes plain `curl` runs.

| Run | Change over the previous shape | Outcome (both 1.13.19 and 1.14.0 unless noted) |
|-----|--------------------------------|------------------------------------------------|
| V5 | `{"port": [53], "action": "hijack-dns"}` as the **first** route rule | System DNS queries now **reach the DNS engine** (log shows Windows telemetry lookups exchanged), but every engine exchange fails: `dns: exchange failed ... context deadline exceeded`. CPU drops to ~0 (no more dial spinning). Conclusion: hijack works, **UDP DNS upstream dials are black-holed**. |
| V6 | DNS servers switched from UDP (53) to **TLS/DoT (853)** | `nslookup` **resolves**; mixed-proxy http/https to baidu **200 in 0.03–0.5 s** on both versions. The earlier "502 = DNS succeeded" reading was wrong — 502 was the proxy reporting resolution failure. |
| V7 | `--noproxy '*'` for TUN-captured curls; system proxy confirmed enabled (`ProxyEnable=1`) | TUN-captured domain curls still fail, but **instantly** (0.02 s) instead of 7 s — a new, local failure mode. Mixed proxy still 200/302. |
| V8 | info-level logs + TUN-captured curl to the **exact IP** | TUN-captured direct to the exact IP: **200 in 0.03 s** on both versions. Logs show the full flow: `inbound/tun` connection → `outbound/direct` dial, plus DNS packets to the peer hijacked and exchanged. IPv4 TUN capture works end to end. |
| V9 | `curl -4` vs `-6` for TUN-captured domain requests | `-4`: **200** (0.05–0.16 s). `-6`: 000 (0.03–2.1 s). Plain (auto, v6-preferred): 000. The last blocker is the **IPv6 path** (upstream #4178, open). |
| V10 | `dns.strategy: ipv4_only` | **Everything green on both versions**: TUN-captured http/https to baidu 200 (0.06–0.12 s), mixed-proxy http/https 200, `nslookup` resolves, CPU ~0. |
| V11 | **Production shape** (fakeip server + `local` + DoT/DoH + `ipv4_only`) | **Fails on both versions**: system DNS works (client gets the fake IP 198.18.0.6 — the engine answers), but every dial to the fake IP fails (000/502) — 198.18.0.0/15 is not among the Windows auto-route sub-ranges, so fake-ip traffic escapes the TUN and is unroutable. **Conclusion: the Windows emission must not use fakeip.** Also observed: `stack: system` + hard `taskkill /F` left the strict-route WFP filters stranded, black-holing host TCP (curl 000, ping OK); a graceful `taskkill /T` exit removed them and restored the host. |
| V12 | **Fixed emission, production DNS** (DoT/DoH, no fakeip/local, `ipv4_only`) + real trojan node | TUN direct 200, but **mixed-proxy 000** and the DNS engine dead: `FATAL start service: circular server dependency: dns-remote-4 -> dns-remote-5 -> dns-remote-5`. The DoH `final` is domain-hosted; its `domain_resolver` points at itself → sing-box aborts the DNS service at startup (adapter DNS never set; system queries escaped via the physical resolver). **Conclusion: on Windows the resolution anchor (final + every `domain_resolver`) must be an IP-hosted TCP server.** |
| V13 | Anchor fix: `final`/`domain_resolver`/suffix rules → last IP-hosted DoT server (223.5.5.5) | **Everything green, real traffic**: TUN direct 200; **mixed-proxy through the real trojan node → google/youtube 200 (0.27–0.55 s)** — the node's domain host resolved via the anchor, dial escaped, proxy handshake worked; `nslookup` answered via the TUN peer; graceful stop left zero WFP residue and host health 200. This is the emission that landed. The cargo-based live acceptance (G9.14) is still to be run on an MSVC-toolchain host. |

Final working shape (locked 2026-09-03, both 1.13.19 and 1.14.0):

```json
{
  "route": {
    "rules": [
      { "port": [53], "action": "hijack-dns" },
      { "process_name": ["ice-box", "sing-box"], "outbound": "direct" },
      { "action": "sniff" },
      { "protocol": "dns", "action": "hijack-dns" },
      { "ip_cidr": ["<tun-cidr>", "<tun-cidr-v6>"], "action": "reject", "method": "drop" },
      { "ip_is_private": true, "outbound": "direct" },
      { "clash_mode": "global", "outbound": "<global target>" },
      { "clash_mode": "direct", "outbound": "direct" }
    ],
    "auto_detect_interface": true,
    "default_domain_resolver": { "server": "<tcp-dns-server-tag>" }
  },
  "dns": {
    "servers": [
      { "tag": "<tcp-dns-server>", "type": "tls", "server": "223.5.5.5", "server_port": 853 },
      { "tag": "<tcp-dns-server-2>", "type": "tls", "server": "119.29.29.29", "server_port": 853 }
    ],
    "final": "<tcp-dns-server>",
    "strategy": "ipv4_only"
  }
}
```

Requirements and limits of the working shape:

- **Port-53 hijack must be the first route rule.** `{"protocol": "dns"}`
  alone does not match in time on Windows (the 1.13 regression documented
  in #3878); without the port rule the OS resolver queries fall into the
  peer-reject drop (or, without it, into the #4455 self-loop).
- **DNS upstreams must be TCP transports (DoT/DoH).** The core's UDP
  outbound is black-holed by its own TUN on Windows (matches the original
  spike's `listen udp4 :0: An invalid argument`); UDP DNS exchanges always
  time out. TCP dials escape (§1.1 row 1), so DoT/DoH work.
- **The resolution anchor must be an IP-hosted TCP server.** The `final`
  and every server `domain_resolver` must reference a server whose host is
  an IP: a domain-hosted DoH final needs a resolver for its own host, and a
  `domain_resolver` chain that ends in itself is a circular dependency the
  pinned core aborts at startup (`FATAL ... circular server dependency`,
  V12). The emission picks the last IP-hosted server (builtin DoT
  223.5.5.5:853 when the profile has none) and points `final` +
  `domain_resolver` + suffix rules at it (V13: verified against a real
  trojan node with a domain host).
- **`ipv4_only` is required.** IPv6-captured connections fail (#4178,
  open) and v6-preferring clients (curl happy-eyeballs, browsers) fail
  overall instead of falling back; `ipv4_only` keeps every connection on
  the proven IPv4 path.
- `strict_route: true` is fine with this shape (host DNS worked throughout
  V6–V10; the 1.14 WFP DNS malformation seen in item 12 did not appear
  with TCP DNS).
- The peer-reject rule stays as defense-in-depth against #4455.
- **Stopping the core must be graceful-first** (`taskkill /T`, `/F`
  fallback after a bounded wait): the strict-route WFP filters are removed
  on graceful exit only; a stranded filter set black-holes every
  non-loopback TCP connection on the host (V11).

Open items that the working shape does **not** cover (next step, not a
release gate):

- **Production fakeip shape**: **tested and rejected (V11)** — the fake-ip
  range (198.18.0.0/15) is not captured by the Windows auto-route
  sub-ranges, so fake-ip answers are unreachable; the Windows emission
  drops the fakeip server and answers real IPs via TCP-transport DNS.
- **UDP user traffic** (QUIC/HTTP3, games, DoH-from-browsers): unverified
  and likely affected by the same UDP-outbound capture; the documented
  first-release behavior should state IPv4 TCP is proven and UDP is not.
- **Long-running stability** (Wi-Fi roam, hours of uptime): unverified.

Host: `win-espf39otjbu\admin`, elevated, WSL2 NAT environment (physical NIC
index 4, `10.28.10.0/24`, gateway `10.28.10.1`; plus Hyper-V vEthernet
interfaces 18/37). Every run left **zero residue** (verified: no adapter, no
routes via `10.0.0.1/10.0.0.2`, default route intact, DNS resolution OK,
ping 7 ms 0% loss). On 2026-09-03 the topology was re-confirmed as a plain
PC: a single physical Realtek PCIe GbE NIC (index 6) behind a NAT router on
`10.28.10.0/24` — the failures below are therefore not an exotic
VM/dual-vEthernet artifact.

The 2026-08-31 table below stays as the original record; §1.1 and §2
supersede the interpretation of items 9–12.

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

## 2. Root cause (final, 2026-09-03)

The V1–V10 progression converges on three independent failures, all
verified live on the real host:

1. **System DNS is never hijacked by `protocol: dns`.** Windows resolves
   the TUN peer (`10.0.0.2` / IPv6 peer) as the adapter DNS; queries to
   the peer enter the TUN but the protocol-based hijack rule does not
   match in time on Windows (1.13 regression, documented in #3878), so
   the queries fall to the peer-reject drop — or, without the reject
   rule, to `ip_is_private → direct` and the #4455 self-loop. The
   fix/workaround is the L4 `{"port": [53], "action": "hijack-dns"}`
   rule as the first route rule (§1.2 V5).
2. **The core's UDP outbound is captured by its own TUN.** sing-box's UDP
   packet sessions are not interface-bound on Windows (the original spike's
   `listen udp4 :0: An invalid argument`), so UDP dials — including the
   DNS engine's upstream exchanges — follow the route table into the
   TUN sub-ranges and die. TCP dials bind via `auto_detect_interface` and
   escape (§1.1 row 1). Workaround: TCP-transport DNS (DoT/DoH) (§1.2
   V6). This is the one remaining core-level defect that a GUI cannot
   route around for user UDP traffic.
3. **The IPv6 path is broken** (upstream #4178, open): captured IPv6
   connections fail and v6-preferring clients do not fall back to IPv4.
   Workaround: `dns.strategy: ipv4_only` (§1.2 V9–V10).

Windows weak-host routing (a bound socket does not override a more
specific route in the table) is the background condition for #2.

## 3. Decision options (recorded, not deferred silently)

1. **Windows-specific config shape (chosen, landed 2026-09-03)** — the
   pinned 1.13.19 (and 1.14.0) works on Windows with the §1.2 shape:
   port-53 hijack first, TCP-transport DNS, `ipv4_only`. No kernel
   upgrade, no helper, no driver. Landed in `ice-config` /
   `ice-subscription` (Windows-only emission) and the backend (DNS
   ownership, graceful stop); V11 additionally proved the production
   fakeip shape does **not** work on Windows (the 198.18.0.0/15 answers
   are outside the auto-route sub-ranges), so the emission drops fakeip.
   Remaining §1.2 open items: UDP user traffic, longevity.
2. **Upgrade the pinned core** — **tested and rejected (twice)**: 1.14.0
   stable re-tested live on 2026-09-03; the three §2 failures persist
   (only the proxied domain-HTTP failure mode improved, 000 → 502).
   Upgrading re-pins the schema for **all platforms** and requires
   re-running the macOS live gates (G9.12/G9.13), so it is not worth it
   now that the §1.2 shape unlocks the pinned core.
3. **Wait for the upstream fix** — no longer required for the product,
   but the §2 failures (UDP outbound capture, IPv6 path, `protocol: dns`
   regression) are genuine upstream defects. File issues/PRs with the
   §1.2 evidence; nothing blocks ice-box while they are open.
4. **Platform adapter**: bundle a different Windows core (mihomo) for TUN
   only — rejected by the locked "sing-box first, no mihomo fork" decision
   (windows-plan §1); would need an explicit plan amendment.
5. **`bridge` outbound (1.14+, recorded lead)**: 1.14.0 adds an L3 `bridge`
   outbound that forwards TUN traffic directly out of a network interface
   (Windows via WinDivert) — likely the eventual upstream fix for §2
   failure 2 (UDP capture). Not needed to ship; revisit after upstream
   matures it.

## 4. What landed behind the gate

**UI**: `status.tun_ui_hidden` is derived from the backend capability in
`capture.rs` rather than a bare `cfg!`, so the TUN controls are visible now
that the Windows backend reports `supported`; the Settings TUN card, the
Home TUN toggle, the helper-install dialog, and TUN-derived power control
state render normally.

`crates/ice-tun-sys/src/windows.rs` is the production backend
(`create_backend` selects it on Windows; the `ICE_BOX_TUN_WINDOWS_DEV`
opt-in is gone). It already
matches the confirmed host facts: interface-index identity, route matching by
IPv4 interface-IP / IPv6 index, and the sub-range route shape (the backend
observes and journals the locked v4/v6 sub-range sets from §1 item 4). The
v6 netsh probe already uses
`interface=` (item 8) and treats only a confirmed missing interface as
"gone" — any other probe failure fails closed instead of being misread as a
verified missing interface. Everything below was incorporated in the flip:

- **DNS ownership claimed** (item 5 — the adapter DNS is set to the TUN
  peers): the backend journals `dns_before`/`dns_after`
  (`netsh interface ipv4 show dnsservers`, host-free parsed), `verify`
  proves the adapter still owns DNS (`dns_consistent`), and `restore`
  compare-before-restores — a third-party change during the session is
  preserved (the backend never mutates DNS itself; no elevated context).
- **Interface discovery** matches by the requested adapter *name* (which
  the app controls), not by description — the "sing-tun Tunnel" adapter
  description is discovered on this host (§1.1 row 7) without a
  "Wintun"-only matcher.
- **The DNS-server `detour: "direct"` runtime FATAL** is never emitted
  (ice-config has no DNS-server detour emission).
- **The §1.2 shape is a Windows-only emission requirement**, landed in
  `ice-config` (reserved rules: port-53 hijack first + sniff +
  protocol-dns hijack + peer-reject of the TUN sub-ranges before
  `ip_is_private`; `default_domain_resolver` → the TCP `final` tag) and
  `ice-subscription` (clash DNS + URI-list DNS: TCP transports only, no
  fakeip server/rule, no `local` server, `strategy: ipv4_only`,
  `domain_resolver` rewired to the `final` tag). macOS emission is
  unchanged (its DNS uses `local` + no port-53 requirement — the macOS
  gates G9.12/G9.13 must stay green).
- **Graceful stop**: the elevated runner stops the core gracefully
  (`taskkill /T`, no `/F`) with a forced fallback after `TERM_GRACE` —
  V11 showed a stranded strict-route WFP filter set black-holes every
  non-loopback TCP connection on the host; graceful exit removes the
  filters.
- **In-app elevation (2026-09-03)**: TUN transitions require an elevated
  process (adapter creation). There is no installable helper on Windows
  (the privileged daemon is macOS-only); the app relaunches itself via
  `Start-Process -Verb RunAs` (UAC) when the TUN toggle is used from an
  unelevated context — the setting is persisted first, the old instance
  quits through the normal cleanup path, and the elevated successor
  (`--elevated-relaunch`, single-instance lock retry) applies `tun.enabled`
  on startup. A cancelled prompt reverts the setting and surfaces
  `tun.elevation_cancelled`. The acceptance suite still runs from an
  already-elevated context.
- **Cached-profile staleness (2026-09-03, live-host incident)**: the
  desktop reuses the parsed `profile.json` across app versions, so a
  profile parsed by an older binary keeps the old DNS shape (fakeip +
  `local` + UDP) — the V11-broken shape — until the subscription is
  re-fetched. `load_active_profile*` now re-applies the Windows emission
  (`normalize_dns_on`) at load time. Also fixed: `netsh` probes spawned
  `CREATE_NO_WINDOW` (a GUI app flashing a console per probe) and the
  missing-interface detection is now locale-proof — the zh-CN netsh error
  (`此名称的接口未与路由器一起注册`) matched no English marker, so an
  absent adapter failed recovery forever (`recovery_required` loop); the
  interface-listing cross-check is authoritative. The 2026-09-03 live
  retest then exposed that *every* netsh / `route print` parser assumed
  English output (zh-CN `接口 "以太网" 的配置`, `活动路由:`, `已连接`,
  `子网前缀` ...), which made `observe_owned_state` never converge and
  the apply roll back after its retry window — all parsers now parse
  structurally (quoted-name boundaries, numeric columns, IPv4/IPv6 tokens,
  `=====` blocks) instead of by English marker. The same retest also
  exposed that adoption of the elevated core failed on Windows:
  `PidProcess` (ice-core, the adopted-pid handle used by
  `adopt_external`) implemented `try_wait` / terminate / kill for unix
  only and returned `Unsupported` on Windows, so the adopt health probe
  aborted immediately (`try_wait: PidProcess liveness requires a unix
  host`); the release + verification then converged clean and the app
  restarted the normal core ~10s after the elevated one started — while
  the TUN itself was healthy and routing traffic the whole time.
  `PidProcess` now implements the Windows contracts
  (OpenProcess + GetExitCodeProcess liveness with STILL_ACTIVE,
  taskkill `/T` graceful-first terminate, TerminateProcess hard kill;
  ERROR_INVALID_PARAMETER = gone, ERROR_ACCESS_DENIED = alive-not-
  signalable parity with the unix EPERM path). The 2026-09-03/04 live
  retests then isolated the final blocker with targeted diagnostics:
  the verify readiness lock failed on `interface_up` while addresses,
  routes, DNS, and the control path all verified — `interface_state`
  resolved the adapter index by re-parsing `list_interface_names`
  output (bare names) as if it were the raw `netsh interface ipv4 show
  interfaces` table, so the index was always `None` and the identity
  lock failed unconditionally on every Windows host. The index now
  comes from the raw listing table (regression test locks the lesson).
  2026-09-04: first fully healthy live activation — `applied` journal,
  `verify_applied`, Wintun index 25, real DNS through the TUN peers,
  zero ERROR/WARN since adoption.

## Plan B (2026-09-04): scheduled-task elevation — zero UAC prompts

The per-session elevation dance (toggle → UAC relaunch → re-enable) is
gone. A scheduled task `ice-box-tun` (highest privilege, never
auto-triggered) runs the TUN core elevated; the app itself never needs
elevation:

- one-time setup: `schtasks /Create` with the bundled
  `ice-tun-launcher.exe` action — created by the installer (best-effort,
  per-user NSIS is not elevated) or by the app's `ensure_tun_elevation`
  (a single UAC prompt, no app relaunch);
- `start` = `schtasks /Run`; `stop` = the app touches the stop file, the
  elevated launcher does the graceful `taskkill /T` (WM_CLOSE, the
  strict-route WFP filters are removed on this path only), then `schtasks
  /End` hard fallback; liveness = cross-integrity
  `PROCESS_QUERY_LIMITED_INFORMATION` + `GetExitCodeProcess` on the
  handshake pid file;
- the UAC relaunch machinery (relaunch_elevated, `--elevated-relaunch`
  instance lock, frontend relaunch path) was removed; the
  `WindowsElevatedCoreCoordinator` (direct spawn, requires an elevated
  app) remains as the fallback when the task is missing (fail-closed
  `tun.permission_required` — the frontend runs `ensure_tun_elevation`
  first, so the fallback only surfaces if the task vanishes mid-flight).
  `tun_elevation_ready` (status) reports task presence; `schtasks`
  probes are exit-code based (zh-CN output is never parsed).

## 5. Safe-testing protocol (this spike)

Every run: warn-level core logs, short curl timeouts, forced
`taskkill /T /F` cleanup, and a post-run residue + network-health check
(no adapter, no `10.0.0.x` routes, default route intact, DNS resolves).
All runs passed cleanup; no user-visible breakage occurred. The
2026-09-03 re-verification (§1.1, §1.2 V1–V10) followed the same protocol,
driven from WSL via elevated PowerShell (UAC), with the exact pinned
binary and the official 1.14.0 stable. V11 (production fakeip shape) and
the WFP-residue recovery run (graceful `taskkill /T` on a stranded host)
are covered in §1.2 and §4; the host's TCP was restored by the graceful
exit before the run finished.