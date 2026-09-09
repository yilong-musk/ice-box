# TUN capture

Living platform lock for TUN. The product model, capture state machine, status
payload, and `tun-state.json` journal live in [`architecture.md`](architecture.md)
§24. This file records emission, elevation, known limits, and live gates against
the pinned sing-box **1.13.19**.

Status: **landed on macOS and Windows**. Default `tun.enabled = false`. The Home
power button starts or stops the proxy service; the Settings/Home TUN switch only
chooses the **next** start (system proxy vs `tun` inbound). `system_proxy` and
TUN stay mutually exclusive at the OS boundary.

---

## 1. Shared

- Mixed inbound stays in both Diagnostic (Mixed-only) and Tun configs. Automatic
  core start always uses the Diagnostic config, after leftover reclaim and crash
  recovery on the same background worker. If `proxy_service_enabled` is set, the
  first UI frame then runs the same start path as the Home power button (system
  proxy or TUN). Crash recovery still never enables capture.
- Dual-stack tun is **mandatory on macOS** (an IPv4-only tun installs no IPv6
  routes and silently leaks IPv6). Windows forces `dns.strategy: ipv4_only`
  instead (upstream #4178).
- Bypass order for a Tun config: control-path / safety rules → `action: sniff`
  → `clash_mode` → custom and subscription rules. `sniff` must precede every
  domain-matching rule (the sniffed name lands in `metadata.Domain`; sniff never
  rewrites the destination at this pin).
- Crash recovery is fail-closed (`recovery_required`): both capture backends
  stay disabled until an explicit `recover_tun` succeeds. Recovery never enables
  capture.

Live gates (destructive, real host):

| Platform | Script | Notes |
|----------|--------|-------|
| macOS | `scripts/run-acceptance-macos-tun.sh` (G9.12) | Dev runner: `ICE_BOX_TUN_DEV_SUDO=1` |
| macOS helper | same script `--helper` (G9.13) | Installed `ice-helper` path |
| Windows | `scripts/run-acceptance-windows-tun.sh` (G9.14) | Needs an MSVC host (`link.exe`) |

---

## 2. macOS

### 2.1 Inbound shape (locked)

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

`address` is the only address field at this pin (`inet4_address` /
`inet6_address` are FATAL). Inbound `sniff` / `sniff_timeout` are FATAL; use
the route rule `{"action": "sniff"}` (string form). `interface_name` must be
`utun<N>` with a numeric suffix. Collision fallback probes a higher index, else
fail closed. Identity for recovery is **name + utun index**, never "any utun".

Reserved route rules that precede `clash_mode`:

```json
[
  { "process_name": ["ice-box", "sing-box"], "outbound": "direct" },
  { "ip_is_private": true, "outbound": "direct" },
  { "ip_cidr": ["127.0.0.0/8", "::1/128", "169.254.0.0/16", "224.0.0.0/4", "ff00::/8"], "outbound": "direct" },
  { "action": "sniff" }
]
```

### 2.2 Elevation: `ice-helper`

Creating a utun, assigning addresses, and adding routes are privileged.
Production runs the bundled core as root via a small launchd daemon
(`crates/ice-helper`). Native sing-box owns the adapter / addresses / routes /
DNS; `ice-tun-sys` journals and verifies. No network-extension package.

The helper starts and stops the bundled core with an allowlisted
config path, applies validated DNS changes (`SetDns`) so TUN DNS
hijack can run elevated, and truncates its fixed core log in place
(`TruncateCoreLog`) so the app can shrink a root-owned
`/var/log/ice-box-core.log` without a permission error. IPC is one JSON
object per line, 16 KiB cap, protocol version **2**, one request/response
per connection (`crates/ice-tun-sys/src/helper_protocol.rs`):

```json
{"v": 2, "token": "...", "cmd": "status"}
{"v": 2, "token": "...", "cmd": "start", "config": "/abs/path/config.json"}
{"v": 2, "token": "...", "cmd": "stop"}
{"v": 2, "token": "...", "cmd": "set_dns", "service": "Wi-Fi", "servers": ["1.1.1.1"]}
{"v": 2, "token": "...", "cmd": "truncate_core_log"}
```

Auth: peer uid (`getpeereid`) is the **primary** control; the per-installation
token (constant-time) is secondary. `start` config must canonicalize inside
the data dir. The helper then sanitises the JSON (`ice-config-guard`: inbound
/ outbound allowlists, no filesystem-referencing keys except bundled geoip
rule-sets) and starts sing-box from a **root-owned** copy under
`/Library/PrivilegedHelperTools/com.yilong-musk.icebox/run/`, never from the
user-writable file. The core binary path is fixed at install and pinned by
SHA-256 under
`/Library/PrivilegedHelperTools/com.yilong-musk.icebox/`. Socket
`/var/run/ice-box-helper.sock` is world-connectable (`0666`) so the
unelevated app can connect; authorization is on the connection. The token
file is `0600` owned by the installing uid (the daemon runs as root and can
still read it).

`SetDns` is validated in the daemon before `networksetup` runs: the service
name must match `[A-Za-z0-9 ._/ -]` (letters, digits, space, `.`, `_`, `-`,
`/`; names like `USB 10/100/1000 LAN` are allowed, shell punctuation is not),
each server must be an IPv4 or IPv6 literal, at most four servers, and an
empty `servers` list clears the override ("Empty" / DHCP fallback). Invalid
input returns `tun.invalid_argument` and does not mutate the OS.

`create_backend` runner order: `ICE_BOX_TUN_DEV_SUDO` → `SudoCoreCoordinator`;
else helper `status` probe → `HelperCoreCoordinator`; else fail-closed
`DeferredCoreCoordinator` (`tun.permission_required`, no OS mutation).

Install is permanently unsigned: `install_helper` / `uninstall_helper` IPC
prompts the system authorization dialog (`crates/ice-elevate`) and runs
`ice-helper install|uninstall`. Manual equivalent:
`scripts/install-helper-macos.sh` / `uninstall-helper-macos.sh`. The
clean-machine install gate is waived; G9.13 still exercises
install → enable → disable → uninstall.

### 2.3 DNS

sing-box itself does not rewrite `scutil --dns`. When `dns_hijack` is enabled,
the backend points the primary network service's DNS at public resolvers
(elevated, journaled `dns_before` / `dns_after`, compare-before-restore) so
port-53 traffic enters the TUN instead of a LAN resolver on the connected
subnet.

---

## 3. Windows

Windows TUN is **IPv4 TCP only**. Use system proxy when UDP or IPv6 is
required. Do not expect QUIC/HTTP3 on this pin.

### 3.1 Elevation: scheduled task

Adapter creation needs Administrator. There is no installable helper on
Windows. A per-user scheduled task `ice-box-tun` (highest privilege, never
auto-triggered) runs `ice-tun-launcher.exe`:

- one-time setup: `ensure_tun_elevation` (one UAC, no app relaunch, no
  console flash). Refuses standard-user accounts (over-the-shoulder UAC
  would register the task as a different administrator). UAC launches the
  GUI-subsystem `ice-tun-launcher.exe` with `--install --data <dir>
  --user-sid <sid>`. That elevated step copies `ice-tun-launcher.exe`,
  `sing-box.exe`, and `libcronet.dll` (plus `wintun.dll` if present) to
  `%ProgramFiles%\ice-box\` (standard users cannot pre-create that tree),
  takes ownership as Administrators, ACLs the directory and each file
  (SYSTEM + Administrators full; Users read/execute), and registers the
  task from an in-memory XML string via `ITaskService::RegisterTask` so
  the SHA-256 pin of the **protected copies** lives in
  `RegistrationInfo/Description` and `Principal/UserId` is the interactive
  SID. The task `Command` points at the Program Files launcher; `Run`
  refuses `current_exe()` outside that directory. Config is sanitised and
  written to `%ProgramData%\ice-box\run\config.json` (admin-owned) before
  spawn. Core log, pid file, and the sanitised config live in that run
  dir (Users read-only). Graceful stop uses a Global named event with a
  DACL rather than a user-writable stop file. The per-user NSIS installer
  does not create the task (it is not elevated); uninstall deletes the
  task and, with a UAC prompt, the protected copies.
- start = `schtasks /Run` after waiting for a previous instance to exit
  (`/End` is asynchronous; `IgnoreNew` would otherwise drop the new run);
  stop = named stop event + graceful `taskkill /T` (no `/F`) then
  `schtasks /End` fallback;
- liveness = handshake pid file + `PROCESS_QUERY_LIMITED_INFORMATION`;
- `schtasks` probes are exit-code based (locale output is never parsed);
- `WindowsElevatedCoreCoordinator` remains a fail-closed fallback if the task
  vanishes mid-flight (`tun.permission_required`).

Graceful stop is mandatory: a stranded `strict_route` WFP filter set
black-holes every non-loopback TCP connection on the host. Hard `taskkill /F`
is the last resort only.

The wintun driver is embedded in `sing-box.exe`; no side-by-side `wintun.dll`.

### 3.2 Emission shape (locked, Windows-only)

macOS emission is unchanged. Windows `ice-config` / `ice-subscription` emit:

```json
{
  "route": {
    "rules": [
      { "ip_version": 4, "port": [53], "action": "hijack-dns" },
      { "process_name": ["ice-box", "sing-box"], "outbound": "direct" },
      { "action": "sniff" },
      { "ip_cidr": ["<tun-cidr>", "<tun-cidr-v6>"], "action": "reject", "method": "drop" },
      { "network": "udp", "port": [443], "action": "reject" },
      { "ip_is_private": true, "outbound": "direct" },
      { "ip_cidr": ["127.0.0.0/8", "::1/128", "169.254.0.0/16", "224.0.0.0/4", "ff00::/8"], "outbound": "direct" },
      { "clash_mode": "global", "outbound": "<global target>" },
      { "clash_mode": "direct", "outbound": "direct" }
    ],
    "auto_detect_interface": true,
    "default_domain_resolver": { "server": "<ip-hosted-tcp-dns>" }
  },
  "dns": {
    "servers": [
      { "tag": "<ip-hosted-tcp-dns>", "type": "tls", "server": "223.5.5.5", "server_port": 853 }
    ],
    "final": "<ip-hosted-tcp-dns>",
    "strategy": "ipv4_only"
  }
}
```

Requirements:

| Lock | Why |
|------|-----|
| IPv4 port-53 `hijack-dns` first | `protocol: dns` does not match in time on Windows (#3878); queries otherwise hit peer-reject or the #4455 self-loop. IPv6 `:53` is not hijacked (#4178); the peer-reject rule drops it |
| No post-sniff `protocol: dns` hijack | sniff false-positives and UDP source-port reuse (#4199) would feed STUN / mDNS / LLMNR into the DNS engine |
| TCP DNS only (DoT/DoH) | the core's UDP outbound is captured by its own TUN (weak-host routing; `auto_detect_interface` binds TCP only) |
| Resolution anchor is an IP-hosted TCP server | a domain-hosted DoH `final` is a startup FATAL (`circular server dependency`) |
| No `local` DNS server | adapter DNS is the TUN peer; `local` re-enters the TUN |
| No fake-ip | 198.18.0.0/15 is outside Windows auto-route sub-ranges; answers are unroutable |
| `ipv4_only` | IPv6 path is broken (#4178); v6-preferring clients do not fall back |
| UDP 443 reject (ICMP) | QUIC/HTTP3 hangs the browser; ICMP fail-fast reuses IPv4 TCP. macOS does not emit this rule |
| Cached profiles re-normalized on load | an older `profile.json` keeps the pre-lock DNS shape until `normalize_dns_on` runs |

Host probes (`netsh`, `route print`) parse structurally, not by English
markers. The backend journals adapter DNS (`dns_before` / `dns_after`) and
compare-before-restores. `PidProcess` on Windows uses `OpenProcess` +
`GetExitCodeProcess` for adopted-pid liveness.

### 3.3 Known limits

| Works | Does not work |
|-------|----------------|
| IPv4 TCP capture (ordinary HTTPS) | **UDP user traffic** (QUIC/HTTP3, games, browser DoH) |
| System DNS via the TUN peer (DoT/DoH) | **IPv6** (upstream #4178) |
| Mixed inbound for diagnostics | **fake-ip** DNS |

A GUI or settings change cannot fix UDP capture on this pin. Upgrading the
bundled core to 1.14.0 was tested and rejected (same three failures; would
re-pin every platform). mihomo as a Windows-only core is rejected (sing-box
first). Revisit after upstream matures the 1.14 `bridge` outbound.

Long-running stability (Wi-Fi roam, hours of uptime) is unverified.

---

## 4. Still open

- G9.14 cargo live acceptance on a host with an MSVC toolchain.
- macOS: Wi-Fi switch / `auto_detect_interface` survival; MTU 9000 on real
  VPN/QUIC paths.
- Windows UDP and IPv6: wait on upstream; do not paper over them in the GUI.
