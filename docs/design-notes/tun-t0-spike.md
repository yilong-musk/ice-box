# TUN T0 spike results — accepted sing-box JSON and host prerequisites

Status: **T0 design note** (recorded alongside the architecture amendment, `docs/architecture.md` §24).

Scope: feasibility spike against the exact bundled core **sing-box 1.13.19**
(revision `b5ebaa1fc0f2b94256180b95468e73ef53caa27d`, `darwin-arm64`, tags
`with_gvisor,with_quic,...`), run live with root on the development macOS host.
Anything not tested against that exact binary is an open item, not a fact.

## 1. Accepted TUN inbound schema (macOS, bundled 1.13.19)

Verified with `sing-box check` and live starts against the bundled binary:

| Field | Accepted | Notes |
|-------|----------|-------|
| `address` (listable CIDR) | ✅ | **The** address field at this pin. IPv4+IPv6 may be mixed in one list. |
| `inet4_address` / `inet6_address` | ❌ | FATAL `legacy tun address fields are deprecated in sing-box 1.10.0 and removed in sing-box 1.12.0` (source: `option/tun.go` marks them `Deprecated: merged to Address`). |
| `interface_name` | ✅ | macOS requires a numeric suffix: `utun<N>` (sing-tun `fmt.Sscanf("utun%d")`). Bare `utun` → FATAL `bad tun name`. |
| `mtu` | ✅ | 9000 verified live (adapter up, traffic OK; no fragmentation issue observed). |
| `stack` | ✅ | `gvisor` / `system` / `mixed` all pass check. gvisor is the first-release default. |
| `auto_route` | ✅ | macOS installs **sub-ranges** `1.0.0.0/8, 2.0.0.0/7, 4.0.0.0/6, 8.0.0.0/5, 16.0.0.0/4, 32.0.0.0/3, 64.0.0.0/2, 128.0.0.0/1` (+ IPv6 `100::/8 … 8000::/1`) instead of `0.0.0.0/0` (`autoRouteUseSubRanges = runtime.GOOS == "darwin"`). Existing host routes (e.g. `127.0.0.0/8` on `lo0`) keep winning over the sub-ranges. |
| `strict_route` | ✅ accepted | No extra darwin behavior at this pin (strict-route handling is Linux/Windows); keep for parity. |
| `route_address` / `route_exclude_address` | ✅ | Sub-ranges minus excludes are installed (observed live: `10.0.0.0/8` exclusion splits `8.0.0.0/5` into `8.0.0.0/7` + `11.0.0.0/8` + `12.0.0.0/6`; `127.0.0.0/8` splits `64.0.0.0/2`; `169.254.0.0/16` splits `128.0.0.0/1`). Excluded ranges keep using the pre-existing default route. |
| `route_address_set` / `route_exclude_address_set` | ✅ | Requires the referenced local rule-set (`.srs`) to exist at start; `check` parses local rule-sets. |
| `loopback_address` | ✅ | Extra loopback exclusion. |
| `include_interface` / `exclude_interface` | ✅ | Per-interface, not per-process. |
| `exclude_mptcp`, `udp_timeout`, `endpoint_independent_nat` | ✅ | |
| `sniff` / `sniff_timeout` / `sniff_override_destination` (inbound) | ❌ | FATAL `legacy inbound fields are deprecated in sing-box 1.11.0 and removed in sing-box 1.13.0` — use the route rule `{"action": "sniff"}` (string form; `{"action": {"type": "sniff"}}` is rejected by this pin). |
| `gso` / `proxy_protocol` (legacy inbound) | ❌ | Removed at this pin. |

Route rules verified live: `process_name`, `clash_mode`, `ip_is_private`,
`ip_cidr`, `domain_suffix`, `action: "sniff"` (string), `rule_set`.

### 1.1 Sniff semantics at this pin (verified in source + live)

- The sniff action never rewrites the destination: `RouteActionSniff` has no
  configurable `override_destination` at this pin (`option/rule_action.go`:
  only `sniffer` / `timeout`; the runtime field is deprecated/internal), and
  `route/route.go` only rewrites `Destination` when that internal flag is set.
- The sniffed name lands in `metadata.Domain`, and `DomainItem::Match`
  prefers `metadata.Domain` over the destination — so **`action: sniff` must
  precede every domain-matching rule** (`domain`, `domain_suffix`,
  `domain_keyword`, ...). Live proof: with `sniff` after a `domain_suffix`
  block rule, the rule never matched; with `sniff` first, the log shows
  `match[2] => sniff` → `sniffed protocol: tls, domain: example.com` →
  `match[3] domain_suffix=example.com => route(block)` and the connection is
  dropped.

## 2. macOS permission model (live host)

- Unprivileged start **fails at adapter creation**: `configure tun interface: Connect: operation not permitted` (utun via `AF_SYSTEM` control socket), even with `auto_route: false`.
- Creating a utun, assigning addresses (SIOCAIFADDR), and adding routes (raw `AF_ROUTE` socket) are **privileged** on macOS with this core.
- Root start verified live: `inbound/tun[tun-in]: started at utun420`, addresses + routes installed, traffic flows.
- → **Native sing-box path requires an elevated execution context.** T0 decision (locked): a small **privileged helper daemon** (launchd, installed/authorized once — on the permanently unsigned release through the system authorization dialog) runs `sing-box run -c <config>` as root; native sing-box owns the adapter / addresses / routes / DNS; `ice-tun-sys` coordinates, journals, and verifies. No network-extension package for the first release. (A `sudo` wrapper is dev-only; network extension is the documented alternative if the helper is ever rejected.)
- Existing utun interfaces `utun0..5` from other software were present on the test host: recovery must verify identity by the exact interface name + id recorded in the journal, never by "any utun".

## 3. DNS behavior (macOS, native path) — live-confirmed

- sing-tun darwin performs **no OS DNS mutation**: `scutil --dns` diff before/after TUN start and after SIGTERM = identical (no OS DNS hijack; only `dscacheutil -flushcache` runs on close).
- DNS interception happens at the **sing-box router** for tunneled traffic: `dig @8.8.8.8` (public resolver, captured) was answered through the DNS module; `dig` via the system resolver (192.168.5.1, LAN → excluded from capture) went direct. No loop, no leak.
- → macOS backend performs no DNS operation; journal `dns_before` / `dns_after` stay absent; `dns_hijack` settings flag is a no-op on macOS.

## 4. Self-traffic / control path (macOS, native path) — live-confirmed

- sing-box's own outbound dials are bound to the real default interface by `auto_detect_interface` → no self-loop (traffic tests succeeded under capture).
- Loopback control works while TUN is active: Clash API `127.0.0.1:29090` responded during capture; the mixed inbound proxy fetch succeeded.
- **`process_name` matching works standalone on macOS** (kernel pcblist searcher): live log shows `router: found process path: ...Chrome Helper..., user: supreulu` and `match[0] process_name=curl => route(direct)` for both IPv4 and IPv6 captured connections.
- Control-path bypass: first-position route rule `{"process_name": ["ice-box", ...], "outbound": "direct"}`; subscription fetch and geoip refresh exit direct.

## 5. Locked first-release Tun config shape (dual-stack)

```json
{
  "type": "tun",
  "tag": "tun-in",
  "interface_name": "utun420",
  "address": ["10.0.0.1/30", "fdfe:dcba:9876::1/126"],
  "mtu": 9000,
  "auto_route": true,
  "strict_route": true,
  "stack": "gvisor",
  "route_exclude_address": [
    "192.168.0.0/16", "10.0.0.0/8", "172.16.0.0/12",
    "127.0.0.0/8", "169.254.0.0/16", "224.0.0.0/4",
    "fe80::/10", "fc00::/7"
  ],
  "loopback_address": ["127.0.0.1", "::1"]
}
```

- **Dual-stack is mandatory, not optional** (spike finding): with an IPv4-only
  `address` list, sing-tun installs **no IPv6 routes** and all IPv6 traffic
  silently bypasses the proxy via the real gateway (`route -n get -inet6 …` →
  `en0`; python's IPv6-first connections went direct). With the ULA
  `fdfe:dcba:9876::1/126` the IPv6 sub-ranges are installed (observed:
  `100::/8 … 8000::/2, c000::/3, …` with `fe80::/10` + `fc00::/7` excluded)
  and IPv6 follows the same rules (`curl -6` direct via process_name;
  python IPv6-only blocked). The ULA gateway is inside the excluded
  `fc00::/7`, so it stays reachable.
- `interface_name`: `utun420` verified free and working on the host; keep a
  documented collision fallback (probe a higher index, else fail closed).
- The reserved bypass route rules that precede `clash_mode` in a `Tun` config:

```json
[
  { "process_name": ["ice-box", "sing-box"], "outbound": "direct" },
  { "ip_is_private": true, "outbound": "direct" },
  { "ip_cidr": ["127.0.0.0/8", "::1/128", "169.254.0.0/16", "224.0.0.0/4", "ff00::/8"], "outbound": "direct" },
  { "action": "sniff" },
  { "clash_mode": "global", "outbound": "<global target>" },
  { "clash_mode": "direct", "outbound": "direct" }
]
```

- `action: sniff` sits **before** `clash_mode` and all custom/subscription
  domain rules (see §1.1). `process_name` and the private/loopback safety
  rules sit first so the control path and local traffic are never sniffed.

## 6. Live-host checkpoints (macOS spike) — results

- Adapter: `utun420` UP, mtu 9000, `10.0.0.1 --> 10.0.0.1 /30` (+ ULA `/126`, link-local).
- Routes: sub-ranges minus excludes, all via utun420; excludes fall through to the real default route.
- System DNS: unchanged at start and after SIGTERM (no OS DNS hijack).
- Loopback Clash API: healthy while TUN active.
- Mixed inbound proxy fetch: 200.
- TUN-captured direct fetch: 200; `process_name` bypass beats the block rule.
- Non-bypassed client (python) forced IPv4 and forced IPv6: **blocked** after sniff (log: `match[3] domain_suffix=example.com => route(block)`).
- DNS via public resolver (8.8.8.8, captured → DNS module) and via LAN resolver (direct): both answered.
- SIGHUP reload: process alive, utun420 re-created, routes re-installed, Clash API healthy (bounded interruption only).
- SIGTERM: process exits, sing-box removes routes + interface (0 routes remaining), DNS unchanged.
- **kill -9: interface removed by the kernel (fd close) and the routes were flushed with it (0 routes referencing utun420 2 s later).** macOS removes utun routes when the interface disappears; the journal + verification remain the recovery safety net and the contract for platforms that do leave residue (Windows).
- Restart over residue: start succeeds (sing-tun EEXIST → delete + re-add path, exercised implicitly).

## 7. Open items (recorded, not deferred silently)

1. **Windows T0 host spike** (blocker for T1 completion): WinTUN driver
   discovery / install permission (admin?), adapter creation without
   elevation, cleanup after normal stop and task kill, UAC/helper selection,
   NSIS install/uninstall residue.
2. macOS interface-name collision handling for `utun420` (fallback probe).
3. Long-running behavior: adapter/route survival across network changes
   (Wi-Fi switch) with `auto_detect_interface` — add to live acceptance.
4. MTU 9000 across real-world VPN/QUIC paths — add to live acceptance.

## 8. T0 fault-injection recovery (host-free, done)

`crates/ice-tun-sys` (new, T0) provides the journal model (`TunJournal`),
the backend contract (`TunBackend`: `capability` / `prepare` / `apply` /
`verify` / `restore` / `recover`), the startup recovery driver
(`RecoveryDriver`: owner-token check, never enables capture, persists
`clean` only after verification), and a host-free `FakeTunBackend` with
scripted failures at every journaled mutation boundary. Test suite
(`tests/recovery_fault_injection.rs`, 19 tests) proves: crash after every
enable mutation converges to `clean`; partial cleanup + retry converges;
stuck/unverifiable resources fail closed to `recovery_required` until an
explicit retry; foreign journals are never touched; DNS compare-before-
restore preserves external changes; recovery re-runs are no-ops after
`clean`. Backends for macOS / Windows land in T2.