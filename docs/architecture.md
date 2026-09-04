# ice-box Architecture (v1)

This document is the **implementation spec** for v1. Code must follow it; if the implementation deviates, update the document first, then the code.

Status: **macOS + Windows v1 implemented** (start/stop, system proxy on both platforms, Clash/sing-box
subscription parsing, hot reload for rule/subscription changes, mode switching (rebuild + reload,
SIGHUP on macOS / restart on Windows) on both platforms, nodes/traffic UI); Windows CI, NSIS installer
and Windows acceptance in place. **TUN capture in progress** (§24, per `docs/tun-mode-plan.md`:
only T0 complete — shared feasibility locks and the host-free journaled recovery core in
`crates/ice-tun-sys`; T1+ pending: no `TunSettings`, config generation, `CaptureController`,
IPC, or platform backend integration yet).

---

## 1. Product scope

### 1.1 What it does

| Item | Decision |
|------|----------|
| Platforms | macOS, Windows (desktop) |
| Shell | Tauri 2 + React (TypeScript) |
| Core | sing-box as an **independent subprocess** |
| Proxy mode | System HTTP / HTTPS / SOCKS (pointing at the local mixed inbound) |
| Subscriptions | Import, list, **single active subscription** (switching switches the entire routing), update, delete |
| Subscription formats | **sing-box JSON first**; **Clash compatible** (YAML / common subscription bodies; incl. proxy-groups / rules / dns); **proxy URI list** (vless / vmess / trojan / ss / hysteria / hysteria2 / tuic / socks / http / wireguard share links) |
| Runtime updates | **sing-box hot reload first**; restart the process on failure |
| Config ownership | UI generates the final `config.json`; sing-box never reads subscription URLs directly |

### 1.2 Explicitly out of scope (v1)

- Linux, iOS, Android
- ~~TUN / global transparent proxy / per-app proxying~~ → **TUN capture is in scope as a second ingress path (see §24, TUN slice)**; per-app proxying remains out of scope
- Visual rule editor, drag-and-drop policy group orchestration
- Full subscription ecosystem (provider-specific dashboards, traffic queries, expiry reminders, etc.)
- Multi-user / remote control / exposing the clash API beyond the local machine
- Electron, or embedding the core into the UI process

Scheduled auto-refresh of subscriptions: **v1 may offer "interval refresh" as an optional follow-up**; the first implementation slice relies on **manual updates**.

---

## 2. Design principles

1. **UI never touches the protocol.** React only calls Tauri commands; node parsing, merging, process and system proxy all live in Rust.
2. **`src-tauri` stays thin.** It assembles crates, IPC, paths and tray; business logic lives in `crates/*`.
3. **sing-box is the only forwarding engine.** Do not re-implement outbound protocols inside ice-box.
4. **Config layering.** Subscription raw bytes, normalized nodes, local template and final runtime config are stored separately; never overwrite the subscription cache with the final JSON.
5. **Failures can roll back.** The system proxy must be backed up before being set; the stop and crash paths must attempt a restore.
6. **Hot reload does not touch the system proxy.** Only re-apply the system proxy when the inbound address/port changed.
7. **Correctness before fancy.** v1 hard-codes a single merge policy instead of a configurable multi-policy matrix.

---

## 3. System context

```
┌─────────────┐     HTTPS      ┌─────────────────┐
│  Sub URL    │◄──────────────►│ ice-subscription │
└─────────────┘                └────────┬────────┘
                                        │ NormalizedOutbound[]
                                        ▼
┌─────────────┐   invoke()    ┌─────────────────┐     spawn/reload      ┌──────────┐
│ React UI    │◄─────────────►│ src-tauri shell │◄────────────────────►│ sing-box │
│ Tray        │               │ + ice-core      │     127.0.0.1:mixed   └────┬─────┘
└─────────────┘               │ + ice-config    │                            │
                              │ + ice-proxy-sys │     system proxy API       │
                              └────────┬────────┘◄───────────────────────────┘
                                       │
                                       ▼
                          app data dir (config / backup / logs)
```

Traffic path (system proxy mode):

```
Browser / system HTTP client
    → OS system proxy (127.0.0.1:mixed_port)
    → sing-box mixed inbound
    → selector `proxy` → concrete outbound
    → remote
```

When ice-box fetches subscriptions itself, it must **bypass the system proxy** (direct connection), so that "proxy not ready / wrong node" can never block subscription updates.

---

## 4. Process and packaging model

### 4.1 Processes

| Process | Role |
|---------|------|
| `ice-box` (Tauri) | UI, tray, orchestration, writes config, changes system proxy |
| `sing-box` | Forwarding, rules, clash API |

- Never link sing-box into the UI process.
- One app instance manages exactly **one** sing-box subprocess.
- Single instance recommended: a second launch focuses the existing window (later via lockfile / Tauri plugin).

### 4.2 Lifecycle and tray (v1 behavior)

| User action | Behavior |
|-------------|----------|
| Close window | **Hide to tray**, core keeps its current state |
| Tray "Quit" | Stop flow (restore proxy first, then kill core) then exit |
| Crash / killed with `kill -9` | Detected on next launch: if `proxy-backup.json` exists and is marked "applied", attempt restore at startup (see §10) |

v1 does **not** do "UI exits, core keeps running as a service". If needed later, that requires a separate helper and is out of scope for this version.

### 4.3 sing-box binary

Source: `third_party/sing-box/<target>/sing-box[.exe]`, copied at build time to `apps/desktop/src-tauri/resources/` and packaged as a Tauri resource (flat file name `sing-box` / `sing-box.exe`).

Runtime resolution order:

1. Dev: repo `third_party/sing-box/<current-target>/`
2. Release: flat `sing-box[.exe]` under the Tauri `resource_dir` (or `resource_dir/<target>/`)
3. Neither found → `Error`, UI hints that the core is missing (`core.not_found`)

**Bundled version (locked): `1.13.19`** (see `third_party/sing-box/VERSION`, `ice_config::ENGINE_COMPAT_CORE_VERSION`, and `ice_core::BUNDLED_SINGBOX_VERSION` mirroring it).

Version policy: config generation is constrained by that version's JSON schema (v1 uses structural validation + startup healthcheck, no full schema compilation). Fetch script: `scripts/fetch-singbox.sh`; pre-bundle prep: `scripts/prepare-singbox-resource.sh`.

---

## 5. Repository structure and crate dependencies

```
ice-box/
├── apps/desktop/                 # the only runnable artifact
│   ├── src/                      # React
│   └── src-tauri/                # thin shell
├── crates/
│   ├── ice-core/
│   ├── ice-proxy-sys/
│   ├── ice-tun-sys/              # TUN ownership, platform permission, recovery journal (§24.4)
│   ├── ice-config/
│   ├── ice-subscription/
│   └── ice-engine/               # config engine facade (§22)
├── third_party/sing-box/
├── configs/examples/
└── docs/architecture.md
```

Dependency direction (reverse dependencies forbidden):

```
ice-box (src-tauri)
  ├── ice-core
  ├── ice-proxy-sys
  ├── ice-tun-sys               # TUN journal + platform backend; never touches system proxy
  ├── ice-config
  └── ice-subscription
        └── ice-config          # shared types such as NormalizedOutbound only

ice-engine                      # facade over ice-config + ice-subscription (§22)
  ├── ice-config
  └── ice-subscription

ice-core ──×── ice-subscription   # no direct dependency; orchestrated by the shell
ice-proxy-sys ──×── ice-core
ice-tun-sys ──×── ice-core        # no direct dependency; orchestrated by the shell
ice-tun-sys ──×── ice-proxy-sys   # TUN state never reuses system-proxy backup data
```

If shared DTOs keep growing, extract `ice-types`; for v1 they stay in `ice-config`.

---

## 6. Runtime directory and file contract

Root path:

- macOS: `~/Library/Application Support/ice-box/`
- Windows: `%APPDATA%\ice-box\`

(Use Tauri `app_data_dir` at implementation time; never hard-code user names.)

```
<app_data>/
├── settings.json                 # app settings (not sing-box)
├── config.json                   # final config handed to sing-box
├── config.json.bak               # last successfully run config (for hot reload failure rollback)
├── proxy-backup.json             # system proxy backup + applied flag
├── tun-state.json                # TUN mutation journal + ownership records (§24.4)
├── sing-box.pid                  # core subprocess pid (unparseable content = no process)
├── subscriptions/
│   ├── index.json                # subscription metadata list
│   └── <uuid>/
│       ├── meta.json             # redundant copy matching the index entry (for per-entry repair)
│       ├── raw                     # raw bytes of the last successful fetch (no extension or .txt)
│       ├── nodes.json              # normalized NormalizedOutbound[] cache
│       └── profile.json            # full normalized result (nodes + policy groups + route + dns + parse_stats)
├── geoip/                         # bundled geoip-{code}.srs rule-sets (for routing GEOIP)
└── logs/
    ├── ice-box.log               # app log
    └── sing-box.log              # core stdout/stderr redirect
```

### 6.1 `settings.json` (v1)

```json
{
  "mixed_listen": "127.0.0.1",
  "mixed_port": 17890,
  "clash_api_listen": "127.0.0.1",
  "clash_api_port": 19090,
  "selected_tag": null,
  "auto_set_system_proxy": false,
  "allow_lan": false
}
```

- `clash_api_*` **binds 127.0.0.1 only**; `0.0.0.0` is forbidden.
- v1 may run without a clash API secret, but if enabled it must be local-only.
- `auto_set_system_proxy`: retained in `settings.json` for serde compatibility only. The product
  no longer uses this flag to apply or skip the OS proxy. Default is `false`. System proxy is
  toggled from the home page (**启动代理服务** / **停止代理服务**). Missing `settings.json` uses
  this default and does not create the file.
- `allow_lan`: LAN sharing switch (default `false`, read compatibly from older settings.json). When on, the Mixed inbound binds `0.0.0.0`, while the system proxy / healthcheck still use `127.0.0.1`; the Clash API stays local-only.
- TUN capture settings are added by the TUN slice (§24.1); missing TUN fields load as disabled
  (legacy `settings.json` files are unchanged).

### 6.2 `proxy-backup.json`

```json
{
  "applied": true,
  "applied_at": "2026-08-22T03:00:00Z",
  "endpoints": { "http_host": "127.0.0.1", "http_port": 17890 },
  "backup": { "enabled": false, "http": null, "https": null, "socks": null, "extra": {} }
}
```

- `applied: true` means **the current system proxy was set by us**; exit/crash recovery must `restore(backup)`.
- After a successful restore, set `applied` to `false`; do not delete the file (for auditability).

### 6.3 Write rules

- All JSON is written to a temp file first, then `rename`d (atomic replacement where possible).
- Trigger reload only after `config.json` was updated successfully.
- Subscription updates: write `raw` + `nodes.json` first, then update `index.json`.

---

## 7. State machine

### 7.1 Core state

```
          start()
Stopped ──────────► Starting ──success──► Running
                      │
                      └──failure──► Error
                                   │
                    stop()/retry start()
                                   ▼
Running ──stop()──► Stopping ──► Stopped
Running ──reload failed and restart failed──► Error
```

Illegal transitions are rejected outright (error returned, state unchanged), e.g. `reload` while `Stopped`.

| State | UI meaning | Allowed operations |
|-------|------------|--------------------|
| `Stopped` | Not running | start, change subscriptions/settings |
| `Starting` | Starting | none (button disabled) |
| `Running` | Proxying | stop, apply (hot reload), update subscriptions |
| `Stopping` | Stopping | none |
| `Error` | Failed; core should be gone or unusable | stop (idempotent cleanup), start |

`CoreState` fields:

- `status`
- `message`: last error/hint shown to humans
- `inbound_host` / `inbound_port`: the currently effective mixed inbound (null while Stopped)

### 7.2 Coupling of system proxy and core

The system proxy is **not** an independent process lifecycle, but it **is** user-toggled
separately from the core:

- **App launch** starts the core only (`start_core`); does **not** apply the OS proxy.
- **Home「启动代理服务」** (`start`) ensures the core is Running, then applies the OS proxy.
- **Home「停止代理服务」** (`stop_system_proxy`) restores the OS proxy and **keeps the core Running**.
- **App quit** (`stop` / `graceful_stop`) restores the OS proxy if `applied == true`, then stops the core.
- Apply while Running re-syncs the OS proxy **only if** it is already applied on disk (port change).
- If enable fails after the core is Running: keep `Running`, surface a warning; mixed inbound stays usable.

---

## 8. Key sequences

### 8.1 Start (core only)

```
1. If already Running/Starting → reject (or no-op for app-launch path)
2. Generate config.json (keep the old file as .bak); empty nodes → direct-only config
3. status = Starting
4. spawn sing-box -c config.json (log goes to sing-box.log)
5. Healthcheck: TCP connect (not HTTP) to the **clash API listen**; timeout **5000 ms**, poll interval 100 ms (`ice_core::HEALTHCHECK_TIMEOUT`)
6. Healthcheck failed → kill process → Error
7. status = Running
```

System proxy is **not** part of this sequence. See §8.1b / §7.2.

### 8.1b Enable / disable system proxy (home page)

```
Enable (启动代理服务):
1. If core not Running → run §8.1 first
2. Backup the system proxy (if not yet applied) → write proxy-backup.json
3. Apply the system proxy → applied=true
4. Apply failed → restore (must not leave applied: true) → keep Running; surface warning

Disable (停止代理服务):
1. If applied → restore → applied=false
2. Core stays Running
```

### 8.2 Stop (app quit)

```
1. status = Stopping
2. If applied → restore → applied=false
3. Send SIGTERM / Windows equivalent to sing-box; SIGKILL on timeout
4. status = Stopped, clear inbound fields
```

Stop must be **as idempotent as possible**: repeated stops succeed.

### 8.3 Apply (subscription or settings change)

```
1. Merge and generate new_config
2. Validate
3. Write config.json (old file already at .bak)
4. If not currently Running → done (next start uses the new config)
5. If Running:
   a. ice-core.reload()  (see §9)
   b. Healthcheck succeeded:
      - inbound unchanged → keep the system proxy
      - inbound changed → restore, then apply the new port
   c. Healthcheck failed → try reloading back to .bak, or restart the process loading .bak
   d. Still failing → restore the system proxy → Error
```

Orchestration lives in **src-tauri / a future ice-app facade**; `ice-subscription` must not call `ice-core` directly.

---

## 9. Hot reload

### 9.1 Primary approach (locked, slice 3 / sing-box 1.13.19)

sing-box 1.13.19's Clash API `PUT /configs` is a **204 no-op** (does not apply the config), so the only
in-process hot reload is **SIGHUP**: `sing-box run -c <config>` re-validates on SIGHUP and rebuilds the
whole service from the same config path (PID unchanged, listen port briefly rebuilt).

| Item | Decision |
|------|----------|
| Method | Unix: send **SIGHUP** to the sing-box subprocess (`ice_core::SignalReloader`); Windows: go straight to §9.2 restart |
| Trigger | call `core.reload()` after writing the new `config.json` (rule/subscription changes) |
| After success | TCP healthcheck against the clash API port again (same as §16.1) |
| Mode switch | **not** a reload — switched live via Clash API `PATCH /configs` (Slice 4c, §12.2), same on every platform |

**Reload surface = rule/subscription/settings changes only.** Routing mode (Rule/Global/Direct)
never rebuilds the config or touches the process: the generated config always carries the
`clash_mode` rules and the runtime mode is switched with `PATCH /configs` (works identically on
macOS and Windows, no restart needed on either platform).

### 9.2 Fallback

Hot reload failed (signal failure, healthcheck failure):

1. Record the error
2. **Restart the process**: stop the subprocess (**do not restore the system proxy here**, since it is about to come back up) → start with the new `config.json` → healthcheck
3. Windows: wait **`WINDOWS_PORT_RELEASE_WAIT` = 500 ms** before restarting to release the port
4. If the inbound is unchanged, keep the system proxy
5. If the restart also fails: status `Error`, `CoreController::needs_proxy_restore() == true`, the orchestration layer cleans up the proxy per Stop

This document locks in "SIGHUP hot reload first, process restart on failure".

---

## 10. Crash recovery

On app startup (`src-tauri` setup, via `ice_proxy_sys::recover_if_applied`):

1. Read `proxy-backup.json` (skip if missing)
2. If `applied == true`: run `restore`; on success set `applied = false` and write back atomically (**do not delete the file**)
3. Clean up invalid `sing-box.pid` (delete if unparseable); liveness detection and takeover are implemented in the core slice
4. **Never** call `apply` on the startup path

Do not silently re-`apply` the system proxy without the user knowing.

---

## 11. Subscription subsystem

### 11.1 Metadata

```json
{
  "id": "uuid",
  "name": "display name",
  "url": "https://...",
  "enabled": true,
  "format": "sing_box | clash | unknown",
  "node_count": 0,
  "last_updated": null,
  "last_error": null,
  "etag": null,
  "user_agent": null
}
```

`etag` / `Last-Modified` are optional, used for conditional requests.

### 11.2 Import

1. Validate the URL (https preferred; v1 may allow http with a UI warning)
2. HTTP GET, **direct, ignoring the system proxy**, timeout (e.g. 20s), body size cap (e.g. 8 MiB)
3. If the body looks like base64 and decodes to Clash/YAML or JSON, re-detect on the decoded result
4. `detect_format`: **JSON with `outbounds`/`endpoints` → sing-box**; otherwise Clash markers; otherwise Unknown → fail
5. `parse_*` → `NormalizedProfile` (nodes + policy groups + routing + DNS); empty nodes fail, **no success state is written**
6. Persist raw / nodes.json / **profile.json** / meta; the first imported subscription automatically becomes **active**
7. Call Apply to generate `config.json` (hot reload if the user is Running)

Naming: user-specified, otherwise from the `content-disposition` response header / URL path / `subscription-<short id>`.

### 11.3 Format detection details

Fixed order:

1. After trimming starts with `{` and parses as JSON, and has `outbounds` or `endpoints` → `sing_box`
2. Has common Clash keys: `proxies:`, `proxy-groups:`, `mixed-port:` etc. → `clash`
3. Full sing-box config: keep non-leaf outbounds such as `route` / `dns` / `selector`, only strip `inbounds` (local ports are decided by the ice-box template)

### 11.4 Clash compatibility (v2 routing capabilities)

Goal: map `proxies` + `proxy-groups` + `rules` + `dns` to sing-box outbound / route / dns.

**Supported type checklist:**

| Clash `type` | sing-box `type` |
|--------------|-----------------|
| `ss` | `shadowsocks` |
| `vmess` | `vmess` |
| `trojan` | `trojan` |
| `socks` / `socks5` | `socks` |
| `http` | `http` |
| `select` (group) | `selector` |
| `url-test` (group) | `urltest` |
| `fallback` (group) | `fallback` |
| `load-balance` (group) | `loadbalance` |

- Unsupported types (e.g. `ssr`, `hysteria`) are **skipped and counted**; if nothing usable remains after skipping → `sub.empty`
- Group reference resolution order: **groups first, then nodes** (groups may reference groups); unknown members go into `parse_warnings` and are skipped
- Rule support: `DOMAIN` / `DOMAIN-SUFFIX` / `DOMAIN-KEYWORD` / `IP-CIDR` / `IP-CIDR6` / `PROCESS-NAME` → corresponding sing-box route rules; `MATCH` → `route.final`; `RULE-SET` / `GEOSITE` are **skipped + warning** (rule-providers deferred to v2.1)
- **GEOIP**: sing-box removed the `geoip` rule and `geoip.db` in 1.12, so v2 uses **bundled rule-sets** instead: `GEOIP,{CODE}` → `{"rule_set": ["geoip-{code}"]}`, with `route.rule_set` entries of `type: local` (`format: binary`), files from `third_party/sing-geoip/rule-set/` (`scripts/fetch-geoip.sh`, 30 countries, bundled into the app resources); copied to the data directory `geoip/` at runtime; country codes not bundled are **dropped at build time with a log line** (non-blocking). `GEOIP,LAN` / `GEOIP,PRIVATE` → inline `ip_is_private` rules
- Subscription DNS: `nameserver` / `fake-ip-range` / `fake-ip-filter` become sing-box `dns`; `dns.listen` has no sing-box 1.13 equivalent (the `dns-in` inbound type was removed), so it is **dropped + warning**; nameservers with domain addresses get `domain_resolver: "local"` appended with a guaranteed `local` server; fake-ip-filter rules referencing `local` likewise ensure a `local` server exists; `proxy-server-nameserver` entries pointing at local ports are not mapped (self-reference guard)
- Caps: 10k rules, 128 groups, 500 nodes; over the limit → truncate + warning
- Entry point: `ice_subscription::parse_clash_profile` (`clash/` module group: proxies / groups / rules / dns / names)

### 11.5 Single active subscription (v2, breaking change)

**v1's simultaneous multi-subscription behavior is removed.**

- `SubscriptionMeta.active: bool`, at most one `true` globally (the old `enabled` field is read compatibly via an alias; a forced migration runs when reading the index)
- At any moment only the **active** subscription's `profile.json` is read to generate the config; no active subscription (or one with zero usable outbounds) → the generated config falls back to **direct-only** (builtin `direct` / `block` outbounds, every route final `direct`), so Start and Apply always work and the core can run before any subscription is imported
- Switching active = switching the whole set of outbounds / route / dns
- When the active subscription changes, if `selected_tag` does not exist in the new profile it falls back to `default_outbound` (the Clash entry group, preferring `Proxies` / `Final` / the first group), and `settings.json` is written back
- `selected_tag` may point at a **policy group or node** tag; the Clash API `PUT /proxies/{tag}` is unchanged

### 11.6 Update / delete

- `update`: re-fetch; on failure keep the old raw/nodes/profile, only write `last_error`
- `update_all`: serial updates; a single failure does not affect the others
- `remove`: delete the directory; if the removed one was active and no active remains → Start falls back to direct-only mode; if Running, apply reloads to the direct-only config
- No active subscription → Start runs in direct-only mode; if Running, apply reloads to the direct-only config

---

## 12. Config generation (ice-config)

### 12.1 Inputs

- `LocalTemplate`: from `settings.json` (inbounds / clash_api / log cannot be overridden by subscriptions)
- `profile: NormalizedProfile`: the active subscription's profile.json (nodes + policy groups + route + dns)
- `selected_tag`

### 12.2 Output skeleton (v2)

- `log`: info + timestamp; file output is handled by process redirect, no hard dependency on sing-box file logging
- `inbounds`: `mixed` inbound, listen/port from the template; with `allow_lan` on, listen is `0.0.0.0` (LAN sharing), otherwise `mixed_listen`; the subscription's `dns.listen` was dropped during parsing, no `dns-in` inbound is generated
- `outbounds`: leaf nodes → policy groups → `direct` / `block` (injected by default)
- `route`: from profile.route; `route.final` points at `default_outbound` or the group holding `selected_tag`; GEOIP rules expand to local rule-sets (see §11.4), rules for unbundled codes are dropped; when the dns block contains a `local` server, append `default_domain_resolver: "local"`
- `dns`: use profile.dns when present (fake-ip etc.), otherwise a minimal `dns` block
- `experimental.clash_api.external_controller` = the local clash API

### 12.4 Routing modes (Slice 4c — locked)

The generated config **always** carries the full rule set regardless of mode, with two
`clash_mode` rules prepended first (`route.rules[0..2]`):

```json
{ "route": {
    "rules": [
      { "clash_mode": "global", "outbound": "<global target>" },
      { "clash_mode": "direct", "outbound": "direct" },
      ...custom / subscription rules (unchanged)
    ],
    "final": "<rule-mode final>" } }
```

- `<global target>` = the outbound `ProxyMode::Global` routes everything through: the injected
  `proxy` selector when the profile has no groups, else the top group / fallback — so homepage
  node selection keeps working in global mode.
- **Mode switching always rebuilds + reloads.** The pinned sing-box 1.13.19 `NewServer`
  starts with an empty runtime `mode-list` and prepends `default_mode`, so the list is always
  `[<default_mode>]` — a single entry, not `["Rule", "Global", "Direct"]` — and `SetMode`
  silently ignores any `PATCH /configs` targeting another mode. `orchestrate_set_proxy_mode`
  still attempts the `PATCH` and verifies it with `GET /configs` as a forward-compatible
  capability gate (it never fires against the pinned core), so every switch falls back to the
  rebuild + SIGHUP reload/restart path; `running_config_supports_clash_mode` skips even the
  attempt on pre-Slice 4c configs.
- `experimental.clash_api` gains `default_mode` = `settings.proxy_mode` capitalized (membership
  is case-sensitive in sing-box `NewServer`; a lowercase entry would be silently ignored — and,
  were the list to contain it, pollute the reported `mode-list`). **`mode_list` must NOT be
  emitted**: the pinned sing-box 1.13.19 rejects the field (`json: unknown field "mode_list"`).
  `settings.proxy_mode` is the **default mode**: applied at build time, restored after every
  restart because the config is rebuilt on apply.
- **`experimental.cache_file` stays OFF** (locked): `Server.Start()` would restore the cached mode
  and override `default_mode`, breaking mode restoration on restart.
- `route.final` keeps the rule-mode value in all three modes; `clash_mode` rules short-circuit
  before it at match time.
- **Disk `config.json` reflects the switch:** the rebuild + reload path regenerates
  `config.json` (baking the new `default_mode`) before the SIGHUP reload, so after every
  switch the running mode matches `settings.proxy_mode`, which remains authoritative.

Minimal DNS fallback (locked in v1, kept as fallback in v2):

```json
"dns": {
  "servers": [
    { "type": "local", "tag": "local" }
  ],
  "final": "local"
}
```

### 12.3 Validation

- Root object, non-empty inbounds/outbounds
- Every route rule's `outbound` / `route.final` reference must exist among outbound tags; every `rule_set` reference must exist among `route.rule_set` entries, otherwise `config.invalid`
- mixed port must differ from the clash API port
- Ports ∈ 1024-65535 (no privileged ports)
- `selected_tag`, if set, must exist, otherwise fall back to `default_outbound` (Proxies/Final/first selector/first node)
- Run once more before Start; surface together with the sing-box startup failure message

---

## 13. System proxy (ice-proxy-sys)

Interface: `backup` / `apply` / `restore`.

### 13.1 Semantics

- `apply`: HTTP and HTTPS point at mixed; SOCKS points at the same port when the system API allows it (mixed supports SOCKS)
- Bypass list: at least `localhost`, `127.0.0.1`, `::1`; macOS per-service and Windows `<local>`
  follow platform conventions. On Windows WinInet, IPv6 loopback must be `[::1]` — a bare `::1`
  in `ProxyOverride` / `INTERNET_PER_CONN_PROXY_BYPASS` returns `ERROR_INVALID_PARAMETER` (87).
- `restore`: fully write back the backup instead of just "turning the proxy off" (if the user originally had another proxy, restore it)

### 13.2 macOS (implemented, slice 4a)

- Enumerate **enabled** network services via `/usr/sbin/networksetup`, set web / secure web / SOCKS per service
- Bypass: `localhost`, `127.0.0.1`, `::1` (`ice_proxy_sys::BYPASS_COMMON`)
- `backup.extra.services[]`: per-service switch, host, port, bypass list for faithful `restore`
- Failures return `proxy.apply_failed` / `proxy.restore_failed`; **must not** mark `proxy-backup.json` as `applied: true` when apply failed (see `apply_and_record`)
- Real-device gate: `cargo test -p ice-proxy-sys -- --ignored --nocapture` (tag `proxy_sys`)

### 13.3 Windows (implemented, slice 4b)

- Live hive source of truth is WinInet `InternetSetOption(INTERNET_OPTION_PER_CONNECTION_OPTION)`,
  not raw registry writes. `apply` does not dual-write `ProxyEnable` / `ProxyServer` /
  `ProxyOverride`; those keys update as a side effect of the per-connection API. Temp-hive unit
  tests still write the three keys directly because they cannot call live WinInet.
- `apply` sets per-connection flags to `PROXY_TYPE_PROXY | PROXY_TYPE_DIRECT`. Connection type
  is written via `INTERNET_PER_CONN_FLAGS` first (MSDN restore/set rule), then
  `INTERNET_PER_CONN_FLAGS_UI` so Internet Options stays in sync. Live **backup** reads
  `FLAGS_UI` (fall back to `FLAGS`): `FLAGS` may hide WPAD on the current network, and
  restoring that would drop the user's auto-detect checkbox. Live apply gates assert the
  **effective** `FLAGS` bits. WPAD (`PROXY_TYPE_AUTO_DETECT`) and PAC
  (`PROXY_TYPE_AUTO_PROXY_URL`) outrank the manual `ProxyEnable` key; leaving them on is why
  Chrome/Edge can stay DIRECT after Start. The `AutoConfigURL` **string is never written on
  apply** — PAC is disabled only by clearing the flag so restore can turn it back on.
- Coverage is every WinInet connection ice-box can name: the default LAN connection
  (`pszConnection = null`), RAS/VPN entries from `RasEnumEntriesW`, and additional names under
  `HKCU\...\Internet Settings\Connections` (skipping `DefaultConnectionSettings` /
  `SavedLegacySettings`). Each connection is snapshotted and restored independently. A named
  connection that cannot be queried (backup) or written (apply / restore — e.g. VPN deleted,
  stale Connections-key name, or WinInet rejecting a non-ASCII dial-up name) is skipped with a
  warning rather than failing the whole operation; the LAN connection is required, and WinHTTP
  apply/restore still runs after named-connection skips.
- System-proxy endpoints on Windows include SOCKS at the same mixed host/port.
  WinInet has no separate SOCKS checkbox; apply writes a multi-protocol
  `ProxyServer` (`http=host:port;https=host:port;socks=host:port`). `backup` /
  `is_proxy_live_applied` parse that string into `http` / `https` / `socks`.
  WinHTTP still uses a plain `host:port` (HTTP proxy only; no SOCKS).
  Chrome/Edge usually still tunnel HTTPS via the HTTP proxy entry; `socks=` mainly
  helps apps that read the WinInet multi-protocol form.
- Live `backup` must snapshot LAN flags, proxy server, bypass, and `INTERNET_PER_CONN_AUTOCONFIG_URL`,
  plus the WinHTTP default proxy (`WinHttpGetDefaultProxyConfiguration`). If that snapshot cannot
  be taken, `backup` / `apply` fail — do not mutate a live hive you cannot restore. Inference of
  WPAD/PAC from `ProxyEnable` + leftover `AutoConfigURL` is **legacy-only** (pre-upgrade
  `proxy-backup.json` with no `connections` / `per_conn_flags`).
- `backup.extra` carries the registry tri-state (`proxy_enable` / `proxy_server` / `proxy_override`)
  for temp-hive tests and extra round-trip, `per_conn_flags` / `autoconfig_url` for the LAN
  connection, `connections[]` (name, flags, server, bypass, PAC URL) for verbatim restore, and
  `winhttp` (`access_type` / `proxy` / `bypass`).
- `restore` writes the registry tri-state back, then pushes each snapshotted connection through
  the per-connection API and restores the WinHTTP default proxy. Proxy server/bypass use empty
  strings to clear apply (WinInet has no delete); `AutoConfigURL` is written only when the
  snapshot had a PAC URL — `None` leaves the leftover string alone, matching apply. Partial
  `apply` failure rolls back from an apply-start snapshot, matching macOS per-service rollback.
- WinHTTP (`WinHttpSetDefaultProxyConfiguration`) is part of live apply/restore so `curl.exe` and
  other WinHTTP clients follow mixed. The write is machine-default and may return
  `ERROR_ACCESS_DENIED` / `ERROR_PRIVILEGE_NOT_HELD` without elevation; that case is a warning,
  not an apply/restore failure (no-UAC lock). Any other WinHTTP error fails the operation.
  Restore still records the original WinHTTP snapshot so an elevated session can put it back.
- `ProxyServer` (WinInet) = `http=127.0.0.1:mixed_port;https=127.0.0.1:mixed_port;socks=127.0.0.1:mixed_port`
- `ProxyOverride` = bypass list (`localhost`, `127.0.0.1`, `[::1]`, `<local>` via `BYPASS_WINDOWS`)
- After `apply` / `restore`, notify via `InternetSetOption` (`INTERNET_OPTION_SETTINGS_CHANGED` /
  `INTERNET_OPTION_PROXY_SETTINGS_CHANGED` / `INTERNET_OPTION_REFRESH`)
- Registry writes that remain (temp hive, restore tri-state) are per-user (`HKCU`); WinHTTP is
  best-effort without UAC — matches the "no UAC" lock
- The `auto_set_system_proxy` settings gate is removed from Start; the flag remains in
  `settings.json` for serde compatibility (default `false`) and is unused for apply/skip
- Live gates mirror G4.3/G4.4 (`cargo test -p ice-proxy-sys g4_3 -- --ignored` on Windows);
  temp-hive unit tests run on Windows CI
- No WinTUN / elevated UAC driver in v1 **for the system-proxy path**; TUN capture (and its
  WinTUN / UAC requirements) lives in §24 / the TUN slice

### 13.4 Relationship to "local-only mixed"

The system proxy points at `127.0.0.1:mixed_port`. Opening the app starts the **core only**;
the home page **启动代理服务** / **停止代理服务** toggles the OS system proxy while the core
stays up. Quitting the app restores the system proxy (if applied) and then stops the core.

---

## 14. IPC contract (Tauri commands)

Conventions:

- Success returns structured JSON
- Failure uniformly returns `{ "code": string, "message": string }` (locked; Rust: `ice_config::AppError`)
- `code` values in §17; commands hold the main-thread lock briefly; blocking IO (HTTP, spawn) uses async / `spawn_blocking`

### 14.1 Existing

| Command | Description |
|---------|-------------|
| `get_status` | `{ core: CoreState, subscription_count }` |
| `list_subscriptions` | `SubscriptionMeta[]` |

### 14.2 Core

| Command | Description |
|---------|-------------|
| `start_core` | §8.1 — app-launch path; core only |
| `start` | §8.1b enable — ensure core Running, then start the configured capture backend (system proxy, or TUN when `tun.enabled`, plan §2) |
| `stop_system_proxy` | §8.1b disable — disable whichever capture backend is active (restore OS proxy or release TUN); keep core Running |
| `recover_tun` | plan §4.3 — on-demand TUN recovery retry (journal recovery driver; never enables capture); returns an optional warning when cleanup is still uncertain |
| `stop` | §8.2 — app quit: disable TUN capture first, restore OS proxy if applied, then kill core |
| `get_log_view` | `{ n: number }` → text lines: merged app+core logs, warnings/errors and key events only (display filter, log files untouched) |
| `get_runtime_config` | current `config.json` text (read-only) |
| `reveal_data_dir` | open the data directory (opener plugin) |

### 14.3 Settings

| Command | Description |
|---------|-------------|
| `get_settings` / `save_settings` | write `settings.json`; Apply if Running |

### 14.4 Subscriptions

| Command | Description |
|---------|-------------|
| `add_subscription` | `{ url, name? }` |
| `remove_subscription` | `{ id }` |
| `update_subscription` | `{ id }` |
| `update_all_subscriptions` | |
| `set_active_subscription` | `{ id, active }` (activating A automatically deactivates the others) |

`add` / `update` / `set_active_subscription` / `remove` **auto-Apply** after the data change succeeds (hot reload if Running). No second "Apply" click required unless Apply itself failed.

### 14.5 Rules

Rule queries and management target the **currently active subscription** (single-subscription model, §11.5). Rule sets can be large (Clash cap 10k), so queries are always **server-side filtered + paginated**, never the full table over IPC.

| Command | Description |
|---------|-------------|
| `get_rule_overview` | `{ total, disabled, custom, rule_sets, types: [{rule_type, count}] }` |
| `list_rules` | `{ keyword?, type?, disabled?, offset, limit }` → `{ total, offset, limit, items: RuleRow[] }` (`limit` capped at 200) |
| `set_rule_disabled` | `{ fingerprint, disabled }` |
| `add_custom_rule` | `{ rule: object }` (must include `outbound`) |
| `remove_custom_rule` | `{ fingerprint }` |

- Rule identity = **fingerprint** (canonical serialization of the rule JSON); disabled state persists by fingerprint, rules whose content is unchanged keep their state after subscription updates
- Overrides are stored in `rules.json` (data dir root): `{ disabled: [fp], custom: [rule objects] }`; subscription raw bytes are never modified
- At build time: disabled rules are dropped, custom rules are **prepended** (take precedence over subscription rules), then the usual GEOIP expansion and outbound reference validation (`config.invalid`)
- Custom rules persist globally across subscription switches; a rule whose `outbound` / `rule_set` references do not exist in the **new** active subscription is **skipped at build time** (with a log warning) instead of failing the whole build, so switching subscriptions can never break Apply / Start — the rule stays persisted in `rules.json` and resumes as soon as its references exist again
- The three write commands (`set_rule_disabled` / `add_custom_rule` / `remove_custom_rule`) **auto-Apply** after a successful change (hot reload if Running); failures surface via `apply_warning`

---

## 15. Frontend structure (suggested)

```
apps/desktop/src/
├── main.tsx
├── App.tsx                 # layout: status bar + pages
├── api/tauri.ts            # invoke wrapper and types
├── pages/Home.tsx          # core status, system-proxy enable/disable, current node, errors
├── pages/Subscriptions.tsx
├── pages/Logs.tsx
└── pages/Settings.tsx
```

v1 minimal UI set:

- Core follows the app (auto-start on launch, stop on quit); home **启动代理服务** /
  **停止代理服务** toggle OS system proxy only; quit restores proxy if still applied
- Subscription list: name, format, node count / policy group count / rule count / DNS marker, last update, error, **active switch (single-select)**, parse_warnings, update/delete
- Import input (URL)
- Read-only log tail
- Settings: ports, whether to set the system proxy
- Home: last 60 seconds traffic chart (Clash API `/traffic` persistent stream + ring buffer; tab switches keep history)
- Nodes page: switch outbound and run batch latency tests

---

## 16. Logging and observability

- App: `tracing` → `ice-box.log` (`ice_config::init_logging`; append-only without rotation in v1, rotation can come later)
- Core: stdout/stderr → `sing-box.log`, except while TUN capture runs through the privileged helper (macOS production path), where the elevated core's output goes to the helper's fixed root-owned `/var/log/ice-box-core.log`; the log view merges that file in as an extra core source (best-effort, latched on the first helper-managed TUN enable in the app session so a finished TUN session's core lines stay visible; never merged under the dev `sudo` runner)
- UI `get_log_view`: merges the log files and **sorts by time** (same-timestamp lines keep file read order; display lines use a compact timestamp and omit source tags)
- Display filter (UI only, never touches the log files): keep WARN/ERROR/FATAL; keep all app INFO (deliberate key events by developers); core INFO only keeps lifecycle keywords (started / stopped / ready / reload / restart), per-connection traffic noise is dropped; DEBUG/TRACE never shown
- Read a tail window instead of the whole file into memory: at most 3000 lines scanned per source, display cap n ≤ 500
- Sensitive data: subscription URLs may be logged; **node UUIDs / passwords must not be written to info logs** (debug requires an explicit switch, off by default)

A successful healthcheck may log one info line: `sing-box ready on 127.0.0.1:17890`.

### 16.1 Healthcheck (locked, slice 2)

| Item | Decision |
|------|----------|
| Target | `clash_api_listen:clash_api_port` (default `127.0.0.1:19090`) |
| Method | TCP `connect` (confirms the port is listening; HTTP Clash API is left to the hot reload slice) |
| Timeout | **5000 ms** |
| Poll | ~100 ms |
| On failure | kill the subprocess, clear the pid, state `Error`, code `core.healthcheck_failed` |

---

## 17. Error codes

| code | Meaning |
|------|---------|
| `core.not_found` | sing-box binary not found |
| `core.spawn_failed` | failed to launch |
| `core.healthcheck_failed` | healthcheck failed |
| `core.invalid_state` | illegal state transition |
| `config.empty_outbounds` | no usable nodes (selection/test paths; Start/Apply fall back to direct-only) |
| `config.invalid` | generation/validation failed |
| `proxy.apply_failed` / `proxy.restore_failed` | system proxy |
| `sub.fetch_failed` | network/timeout |
| `sub.unknown_format` | unrecognized format |
| `sub.parse_failed` | parse failed |
| `sub.empty` | no nodes |

TUN capture codes (TUN slice, §24):

| code | Meaning |
|------|---------|
| `tun.not_supported` | platform cannot run TUN (`tun_available=false`) |
| `tun.permission_required` | elevation / permission needed, never auto-retried |
| `tun.apply_failed` | capture apply failed |
| `tun.restore_failed` | capture restore failed |
| `tun.healthcheck_failed` | adapter / route / DNS / control-path readiness disagreed |
| `tun.recovery_required` | cleanup unverified; fail-closed until explicit recovery |

UI shows `message`, developers rely on `code`. IPC failure body shape in §14 (`{ code, message }` locked).

---

## 18. Security

- clash API and mixed inbound are `127.0.0.1` only by default
- Subscription requests validate TLS certificates (default reqwest / system roots)
- Never execute arbitrary scripts from subscriptions; parse data only
- Tauri CSP: tightened for production; no dangerous shell plugins unless reveal-dir requires it
- Config file permissions: user-private on macOS (default umask)

v1 does not handle adversarial cases beyond "malicious subscription exhausting memory"; but a body size cap is mandatory.

---

## 19. Implementation slices (suggested order)

Delivery order tied to the architecture:

1. **Paths and settings persistence**, pid, crash-recovery proxy restore
2. **ice-core**: spawn / stop / healthcheck / pid
3. **Hot reload + fallback restart**
4. **ice-proxy-sys** macOS, then Windows
5. **ice-subscription** fetch + sing-box / Clash parsing + single active subscription
6. **Clash parsing** (incrementally per protocol)
7. **React**: subscriptions page + start/stop + logs
8. **Packaging**: bundle sing-box as a resource, installers for both platforms

Every step must be individually `cargo test`-able or manually testable; never roll slices 2-7 into one PR.

---

## 20. Testing strategy

| Layer | Content |
|-------|---------|
| `ice-config` | empty nodes fail; tag conflicts; port conflicts |
| `ice-subscription` | sample JSON / Clash fixture detection and parsing |
| `ice-core` | illegal state machine transitions; mock process can stand in for real sing-box |
| `ice-proxy-sys` | platform tests marked `#[ignore]` or manual checklist; CI never touches the real system proxy by default |
| `ice-config` / ice-box start | default `auto_set_system_proxy` is `false` (legacy field); Start never applies OS proxy; `create_system_proxy()` + defaults must not mutate the OS in CI (macOS/Windows live apply stays `#[ignore]`) |
| Manual | Start then curl the mixed port; Stop then verify the system proxy is restored |

`configs/examples/` serves as the fixture source.

---

## 21. Locked vs. to-be-documented during implementation

**Locked (do not change silently):** platforms (macOS / Windows), Tauri+React, engine crates, subprocess, system proxy, subscription import/management, sing-box first + Clash compatible, hot reload first. TUN locks: §24 (capture model, exclusivity, journal contract, platform locks, bypass policy).

**Details to write back into this document during implementation:**

- ~~Bundled sing-box **exact version**~~ → **written in §4.3 / this table**: `1.13.19`
- ~~reload HTTP method and path~~ → **written in §9.1**: `PUT /configs?force=true`
- Clash supported protocol checklist → **written in §11.4**: ss / vmess / trojan / socks / http
- ~~final JSON of the minimal DNS config~~ → **written in §12.2**: `type: local` / `final: local`
- ~~Windows port release wait on restart~~ → **written in §9.2**: 500 ms
- TUN schema + platform locks → **written in §24.5**; exact JSON in the T0 design note

---

## 22. Config engine facade (ice-engine)

`ice-engine` is the single cross-platform entry point for the config pipeline:
**subscription body → normalized profile → final sing-box config**. It re-exports
the engine surface from `ice-config` (build / validation / settings) and
`ice-subscription` (import / parse / storage) and adds:

- `EngineError` unifying `ConfigError` and `SubscriptionError`
- `import_subscription(raw)` → `(SubscriptionFormat, NormalizedProfile)`
- `build_config(&BuildInput)` → `serde_json::Value`
- `subscription_to_config(raw, template, geoip_dir)` → pretty JSON string
- `ENGINE_COMPAT_CORE_VERSION` — the sing-box version the generator targets
  (`1.13.19`); bundled desktop binaries and any future embedded core must match

Rules:

1. Desktop shell may use the facade or the underlying crates directly; the facade
   exists so future hosts (mobile apps embedding libsing-box) have one documented
   API and cannot accidentally pull desktop-only crates (`ice-core`, `ice-proxy-sys`).
2. The engine must stay free of platform dependencies (process / proxy / TUN live
   in desktop crates). Verify with `cargo tree` after dependency changes.
3. Supported platforms today are **macOS / Windows**. Cross-compilation checks for
   iOS / Android require those targets and an Android NDK; set them up only when
   mobile development starts.
4. `ice_core::BUNDLED_SINGBOX_VERSION` mirrors `ice_config::ENGINE_COMPAT_CORE_VERSION`;
   change the pin in the engine only.

---

## 23. Glossary

| Term | Meaning |
|------|---------|
| Core | the sing-box subprocess |
| Runtime config | `config.json`, the file sing-box actually loads |
| Template | local inbound / clash API / selector, not from subscriptions |
| Normalized node | `NormalizedOutbound`, already a sing-box outbound object |
| Apply | rebuild the runtime config from subscription + template, reload if needed |
| System proxy | the OS's HTTP(S)/SOCKS proxy settings, not TUN |
| Capture backend | how applications enter sing-box: system proxy **or** TUN (§24), never both |
| Diagnostic config | Mixed-only runtime config (automatic core start / stopped service) |
| TUN journal | `tun-state.json`: mutation log + ownership records for capture recovery (§24.4) |
| Capture backend | how applications enter sing-box: system proxy **or** TUN (§24), never both |
| TUN journal | `tun-state.json`: mutation log + ownership records for capture recovery (§24.4) |

---

## 24. TUN capture (TUN slice)

Status: **T0 complete per `docs/tun-mode-plan.md`** (shared/macOS feasibility locks plus the
host-free journaled recovery core in `crates/ice-tun-sys`), **T1 shared complete**
(`TunSettings` in `settings.json`, `CaptureIntent::{Diagnostic, Tun}` config generation,
structural intent validation, `tun_gate` capability preflight), **T2 shared complete**
(macOS backend `MacosTunBackend`: host reads, utun collision fallback, journaled
apply/restore/recover with rollback; `CoreCoordinator` boundary; fail-closed
`UnsupportedTunBackend`), **T3 shared complete** (`CaptureController` in the shell:
active backend + capture state machine, typed status payload, serialized settings
transaction with `settings-pending.json`, startup/watchdog recovery, quit ordering,
`adopt_external` core lifecycle for the elevated runner), **T4 complete** (typed TUN
status/settings in the frontend API, Settings `tun.enabled` switch rendered from
`tun_available`, Home active-backend status with TUN interface, `permission_required`
fallback action and `recovery_required`「重试恢复」action via the `recover_tun` IPC
command, frontend tests green), and **T5 (macOS helper + packaging) landed** (production
privileged helper daemon `crates/ice-helper` with narrow authenticated IPC, app-side
`HelperCoreCoordinator`, `create_backend` wiring, launchd install/uninstall scripts,
entitlements, bundle embedding + CI check, and the G9.13 helper acceptance path (green on an
authenticated host); the app is permanently unsigned — the helper is installed through the
system authorization dialog (in-app `install_helper` / `uninstall_helper` IPC, `crates/ice-elevate`)
or the install script, and the clean-machine gate is explicitly waived for this release.
**Windows T0 complete — `windows_tun_ready` flipped 2026-09-03**: the V1–V11 host spike on a
real Windows 11 host (driven from WSL via elevated PowerShell) locked the §1.2 config shape
(port-53 hijack first, TCP-transport DNS only, `ipv4_only`, peer-reject; no fakeip — the
198.18.0.0/15 answers are unreachable on Windows) and the production Windows backend is
active in `create_backend`: `WindowsTunBackend` with read-only `netsh` / `route print`
host probes and DNS ownership (`dns_before`/`dns_after`, compare-before-restore, verified
adapter DNS), the graceful-stop elevated core runner (WFP filters are removed on graceful
exit only — a stranded filter set black-holes host TCP), and platform emission in
`ice-config`/`ice-subscription`. Windows T1–T5 follow the macOS slice order; the live gate
is `scripts/run-acceptance-windows-tun.sh` (G9.14). See
`docs/design-notes/tun-windows-t0.md`).
This section records the approved product model, state machine, data
contract, and the T0 platform locks. The plan's §2 decision record (capture selection ≠
routing policy) is approved; T0's three open decisions are resolved as follows.

### 24.1 Product model

Traffic capture selection and routing policy are separate concerns:

```text
tun.enabled: true  -> proxy service uses tun
tun.enabled: false -> proxy service uses system_proxy
proxy_mode:  rule | global | direct
```

- The Home page keeps **one** generic proxy-service start/stop control; Settings owns the
  `tun.enabled` switch. `tun.enabled` is the *desired* backend for the next service start; the
  active backend is reported separately in status and owned by the runtime capture controller.
- `system_proxy` and `tun` are mutually exclusive at the OS boundary. Switching `tun.enabled`
  while the service is active is a serialized backend transition: old capture confirmed
  disabled → new capture prepared/enabled → health checks → commit `settings.json`. On failure,
  the old backend is restored; if rollback is uncertain, both backends stay disabled and
  `tun_status=recovery_required` is surfaced (no fallback until cleanup is verified).
- The runtime controller enforces exclusivity: `enable_tun` rejects while a system-proxy record
  is active; `enable_system_proxy` rejects while TUN is `Preparing` / `Enabled` / `Stopping`.
- Default `tun.enabled = false`; no settings migration ever enables TUN implicitly.
- While TUN is active the runtime config contains both `mixed` and `tun` inbounds (Mixed stays
  usable for diagnostics). Automatic core start always uses the **Diagnostic** (Mixed-only)
  config: the TUN inbound exists in `config.json` only while a TUN capture transition is in
  flight or active, so the app's automatic core start can never silently create an adapter or
  install routes.
- Frontend behavior (T4): Settings owns the TUN enable switch, rendered from `tun_available`
  (unavailable → disabled switch + reason; transition in flight → disabled + status hint;
  active → interface shown). Home reports the active backend (`TUN 已接管（utunN）` vs
  system-proxy labels) and keeps the generic power button. `permission_required` offers a
  system-proxy fallback **only when no TUN resource is active**; `recovery_required` replaces
  the fallback with a「重试恢复」action (IPC `recover_tun`, journal recovery driver, never
  enables capture) and blocks activation until recovery succeeds. A configured-but-unavailable
  platform shows the reason and keeps the button disabled instead of a misleading enabled
  state.

### 24.2 Capture state machine

Core `Running` is a prerequisite for capture but is not equivalent to capture `Enabled`.

```text
Capture: Disabled -> Preparing -> Enabled -> Stopping -> Disabled
                         \-> PermissionRequired / Error / RecoveryRequired
```

- `RecoveryRequired` is fail-closed: both capture backends stay disabled and new TUN activation
  is rejected until an explicit recovery attempt succeeds.
- The active backend is owned by one runtime `CaptureController` in `AppState`; every start /
  stop / apply / reload / quit / crash-recovery path reads that controller. No path infers the
  active backend from `tun.enabled`, `settings.json`, or `proxy-backup.json`.
- Transition ordering (bounded traffic interruption allowed; no zero-downtime promise):
  - **Enable TUN:** lock → journal `preparing` → build/validate Tun config → core reload/restart
    with the TUN inbound → verify Clash API + TUN health (`TunHealth` all-ok) → journal
    `applied` → capture `enabled`.
  - **Disable TUN:** journal `restoring` → release capture (native path: core restart to the
    Diagnostic config; helper path: restore routes/DNS/adapter first) → verify no owned
    resource remains → journal `clean` → capture `disabled`. Core may stay Running on the
    Diagnostic config.
  - **Reload (endpoint unchanged):** capture moves to `preparing`; a restart that removes
    resources is treated as disable/re-apply, not a transparent reload.
  - **Topology change** (address / MTU / stack / DNS-interception / route policy): explicit
    disable → validate new Tun config → enable sequence. No in-place topology mutation.
  - **Policy-only change** (rules / nodes / `ProxyMode`): normal core reload while TUN is
    owned; capture returns to `enabled` only after the Clash API + TUN health checks pass.
- Unexpected sing-box exit while TUN is active: the watchdog acquires the orchestration lock,
  marks capture `stopping`, runs the controller's idempotent restore/verify sequence
  immediately, and writes/validates the Diagnostic config so a later automatic start cannot
  recreate TUN from a stale runtime file. If cleanup is uncertain: stop any remaining core,
  keep both backends disabled, persist `RecoveryRequired`, retry on later watchdog ticks and
  the next startup.
- Quit always disables TUN before killing sing-box, with a bounded timeout and a visible
  warning if cleanup cannot be confirmed.

### 24.3 Status payload

```text
traffic_capture: inactive | system_proxy | tun        # derived only from the runtime controller
configured_tun: boolean                                # committed settings desire
tun_status: disabled | preparing | enabled | stopping | permission_required | error | recovery_required
tun_interface: optional string
tun_error: optional stable AppError payload
capture_transition_id: optional opaque identifier
tun_available: boolean
tun_unavailable_reason: optional stable message
```

- `traffic_capture` is `inactive` (no backend claimed) when cleanup is uncertain, while
  `tun_status=recovery_required` blocks fallback and shows the recovery action.
- `configured_tun` is the committed desired setting; it is not set to `true` until an active
  transition succeeds when the service is already running.
- Raw route tables and privileged helper internals are never exposed to the frontend.

### 24.4 TUN mutation journal (`tun-state.json`)

```json
{
  "state": "preparing | applied | restoring | error | recovery_required | clean",
  "transition_id": "...",
  "interface_name": "...",
  "interface_id": "...",
  "addresses": [{"cidr": "...", "owned": true}],
  "routes": [{"destination": "...", "gateway": "...", "metric": 0, "owned": true}],
  "dns_before": {"platform_snapshot": "..."},
  "dns_after": {"platform_snapshot": "..."},
  "owner_token": "ice-box:<installation-id>",
  "last_completed_step": "...",
  "updated_at": "..."
}
```

- Atomic writes; a mutation journal, not a final-state snapshot. `owned: true` is the only
  authorization to remove a resource; an unverified resource is never deleted.
- Written `preparing` + transition ID before the first OS mutation; updated after each
  interface/address/route/DNS mutation with `last_completed_step`.
- DNS restore is compare-before-restore: `dns_before` is restored only when the platform still
  matches `dns_after`; an external DNS change is preserved and produces `recovery_required`,
  never overwritten with stale data. Same ownership check applies to routes and adapter identity.
- System-proxy recovery stays independent and keeps using `proxy-backup.json`; TUN state is
  never derived from it.
- Startup recovery (inside the orchestration lock, after orphan-core reclamation): verify the
  owner token (foreign journal → nothing touched) → resume the idempotent restore from
  `last_completed_step` → mark `clean` only after adapter/routes/DNS verification succeeds →
  otherwise persist `recovery_required` and block new TUN activation. Recovery never enables
  capture, even when `settings.json` has `tun.enabled=true`.

### 24.5 Platform locks (T0)

1. **Schema pin (locked by the exact-binary spike):** the bundled `1.13.19` accepts the TUN
   inbound `address` field (listable prefixes, IPv4+IPv6); `inet4_address` / `inet6_address`
   are **rejected** by this build (`legacy tun address fields ... removed in 1.12.0`). Legacy
   inbound `sniff` / `sniff_timeout` fields are rejected; sniffing moves to a route rule
   `{"action": "sniff"}` (string form). Accepted fields verified by the spike:
   `interface_name` (macOS requires `utun<N>` with a numeric suffix), `mtu`, `auto_route`,
   `strict_route`, `stack` (`gvisor` / `system` / `mixed`), `route_address`,
   `route_exclude_address`, `route_address_set` / `route_exclude_address_set` (rule-set
   references require the `.srs` files to exist at start), `loopback_address`,
   `include_interface` / `exclude_interface`, `exclude_mptcp`, `udp_timeout`. The exact JSON
   shape and defaults are recorded in the T0 design note (`docs/design-notes/tun-t0-spike.md`).
2. **macOS permission model (locked by the live spike):** creating a utun interface, assigning
     addresses, and adding routes are **privileged** on macOS (unprivileged start fails at
     `Connect: operation not permitted`). sing-box must therefore run elevated; the locked
     execution context is a small privileged helper daemon (launchd) that runs the core as root
     (native sing-box owns the
     adapter/addresses/routes/DNS; `ice-tun-sys`
     coordinates and verifies). A network-extension package was not required for the first
     release. **Production helper (T5, landed):** `crates/ice-helper` serves a narrow
     one-frame-per-connection JSON protocol over a root-owned Unix socket — `status` /
     `start {config}` / `stop` only, no binary path / route / interface input ever accepted
     from the client. Security: peer uid (`getpeereid`), per-installation token
     (constant-time compare, root-owned 0644 in the data dir), protocol version, and a
     canonicalized config path inside the installed data dir. The core binary path is fixed
     at install. The app side (`HelperCoreCoordinator`) implements `CoreCoordinator`;
     `create_backend` picks the dev `sudo` opt-in first, then the helper when a read-only
     `status` probe authorizes, else the fail-closed deferred runner. **Install (permanently
     unsigned):** the app never signs or notarizes, so SMAppService is not used; the
     `install_helper` / `uninstall_helper` IPC commands prompt the system authorization dialog
     (`AuthorizationServices`, deprecated-but-functional, `crates/ice-elevate`) and execute
     the helper's own privileged `install` / `uninstall` modes as root — the single shared
     implementation of the install logic (token, plist, pinned SHA-256, launchctl), also
     driven manually via
     `scripts/install-helper-macos.sh` / `uninstall-helper-macos.sh`
     (design note `docs/design-notes/ice-helper-design.md`).
     The clean-machine install/uninstall gate is explicitly waived for this release.
     **Dev path (T3,
     live gate):** until the helper is installed, the
     explicit `ICE_BOX_TUN_DEV_SUDO` opt-in wires `SudoCoreCoordinator`, which runs the core
     as root via `sudo -n` (never prompts; `tun.permission_required` before any OS mutation
     without a cached credential / NOPASSWD rule) and terminates as root with bounded TERM→KILL
     grace, because a non-root shell cannot signal a root-owned process. The destructive live
     suite is `scripts/run-acceptance-macos-tun.sh` (G9.12 via the dev runner, G9.13 via the
     installed helper); the ordinary gates stay non-privileged.
3. **DNS (locked, live-confirmed):** native sing-box on macOS does **not** modify system DNS
   (`scutil --dns` unchanged at start/stop). DNS interception happens at the sing-box router
   for tunneled traffic (UDP/TCP 53 → DNS module); LAN/private resolvers bypass the tunnel via
   `route_exclude_address` (RFC1918/4193, link-local, loopback, multicast). No OS DNS
   operation is performed; `dns_before`/`dns_after` stay absent on the macOS backend.
4. **Dual-stack is mandatory (locked by the live spike):** an IPv4-only `address` list makes
   sing-tun install **no IPv6 routes**, silently leaking all IPv6 traffic through the real
   gateway. The locked tun carries a ULA IPv6 address (`fdfe:dcba:9876::1/126`) so IPv6 is
   captured and follows the same rules; the ULA gateway sits inside the excluded `fc00::/7`.
   The settings `ipv6_address` field is therefore required, not optional.
5. **Ownership (locked):** one owner per resource. On macOS, sing-box owns the adapter,
   addresses, routes, and any DNS state; `ice-tun-sys` records ownership in the journal and
   verifies. No split ownership between sing-box and a helper for the same resource.
6. **Control path (locked, live-confirmed):** loopback Clash API / Mixed are excluded from
   capture by the OS route table plus `loopback_address`; sing-box's own dials are bound to
   the real default interface via `auto_detect_interface` (no self-loop); ice-box's own
   control traffic (subscription fetch, geoip refresh) is routed direct by a first-position
   `process_name` route rule (native darwin process matching verified live in the standalone
   binary). **Sniff ordering:** the sniff action never rewrites destinations at this pin;
   the sniffed domain lands in `metadata.Domain`, so `{"action": "sniff"}` must precede every
   domain-matching rule (subscription rules match only the sniffed domain for IP connections).
   Bypass ordering for a `Tun` config (locked): `process_name` / `ip_is_private` / `ip_cidr`
   safety rules → `action: sniff` → `clash_mode` rules → custom and subscription rules. The
   `Diagnostic` config keeps the existing Mixed-only behavior unchanged.
7. **Crash residue on macOS (locked, live-confirmed):** SIGTERM removes routes + interface
   itself; `kill -9` leaves the interface removed by the kernel and the utun routes flushed
   with it (verified: 0 routes referencing the interface 2 s after `kill -9`). The journal +
   verification stay the recovery safety net and the contract for platforms that leave
   residue (Windows T0 host spike pending).
8. **Windows:** WinTUN driver discovery, UAC / helper behavior, and clean-machine install
   checks are the T0 Windows host spike, to be run on a real Windows host before T1 completes
   (recorded in the design note's open items).

### 24.6 Reserved bypass policy (first release, fixed)

Loopback, link-local, multicast, RFC1918, RFC4193, the Mixed and Clash API endpoints, the TUN
CIDR, DNS resolver traffic, and ice-box/sing-box control traffic take the documented direct
path. Other LAN destinations follow normal routing policy; `allow_lan` does not broaden the
bypass list. IPv4 is mandatory; IPv6 capture is **mandatory as well** (dual-stack tun, §24.5
point 4 — an IPv4-only tun silently leaks IPv6) and is exposed as a capability/limitation in
status and docs, never labeled "all traffic".
