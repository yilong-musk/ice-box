# ice-box Architecture (v1)

This document is the **implementation spec** for v1. Code must follow it; if the implementation deviates, update the document first, then the code.

Status: **macOS + Windows v1 implemented** (start/stop, system proxy on both platforms, Clash/sing-box
subscription parsing, hot reload for rule/subscription changes, live mode switching via Clash API on
both platforms, nodes/traffic UI); Windows CI, NSIS installer and Windows acceptance in place.

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
| Subscription formats | **sing-box JSON first**; **Clash compatible** (YAML / common subscription bodies; incl. proxy-groups / rules / dns) |
| Runtime updates | **sing-box hot reload first**; restart the process on failure |
| Config ownership | UI generates the final `config.json`; sing-box never reads subscription URLs directly |

### 1.2 Explicitly out of scope (v1)

- Linux, iOS, Android
- TUN / global transparent proxy / per-app proxying
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
  ├── ice-config
  └── ice-subscription
        └── ice-config          # shared types such as NormalizedOutbound only

ice-engine                      # facade over ice-config + ice-subscription (§22)
  ├── ice-config
  └── ice-subscription

ice-core ──×── ice-subscription   # no direct dependency; orchestrated by the shell
ice-proxy-sys ──×── ice-core
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
  "auto_set_system_proxy": true,
  "allow_lan": false
}
```

- `clash_api_*` **binds 127.0.0.1 only**; `0.0.0.0` is forbidden.
- v1 may run without a clash API secret, but if enabled it must be local-only.
- `allow_lan`: LAN sharing switch (default `false`, read compatibly from older settings.json). When on, the Mixed inbound binds `0.0.0.0`, while the system proxy / healthcheck still use `127.0.0.1`; the Clash API stays local-only.

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

The system proxy is **not** an independent state machine; it is a side effect of Start/Stop:

- Applied **only as the last step** of entering `Running` (if `auto_set_system_proxy`).
- Restored **as the first step** of leaving `Running` (as long as `applied == true`).
- `Starting` failure: must not leave a system proxy behind.

---

## 8. Key sequences

### 8.1 Start

```
1. If already Running/Starting → reject
2. ice-config: no usable nodes → Error (no process started)
3. Generate config.json (keep the old file as .bak)
4. status = Starting
5. spawn sing-box -c config.json (log goes to sing-box.log)
6. Healthcheck: TCP connect (not HTTP) to the **clash API listen**; timeout **5000 ms**, poll interval 100 ms (`ice_core::HEALTHCHECK_TIMEOUT`)
7. Healthcheck failed → kill process → restore (if already applied) → Error
8. Backup the system proxy (if not yet applied) → write proxy-backup.json
9. Apply the system proxy → applied=true
10. Apply failed → restore → stop core → Error
11. status = Running
```

### 8.2 Stop

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
- At any moment only the **active** subscription's `profile.json` is read to generate the config; no active subscription → `config.empty_outbounds`
- Switching active = switching the whole set of outbounds / route / dns
- When the active subscription changes, if `selected_tag` does not exist in the new profile it falls back to `default_outbound` (the Clash entry group, preferring `Proxies` / `Final` / the first group), and `settings.json` is written back
- `selected_tag` may point at a **policy group or node** tag; the Clash API `PUT /proxies/{tag}` is unchanged

### 11.6 Update / delete

- `update`: re-fetch; on failure keep the old raw/nodes/profile, only write `last_error`
- `update_all`: serial updates; a single failure does not affect the others
- `remove`: delete the directory; if the removed one was active and no active remains → Start not allowed; if Running, stop and prompt
- No active subscription → Start not allowed; if Running, stop and prompt

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
- The runtime mode is switched live via Clash API `PATCH /configs` (`ice_core::set_mode`,
  `orchestrate_set_proxy_mode`): no config rebuild, no reload, no restart, no system proxy churn.
  `PATCH` only changes routing when the running config carries the `clash_mode` rules; on a
  pre-Slice 4c config (or any `PATCH` failure) the switch falls back to rebuild + reload/restart.
- `experimental.clash_api` gains `mode_list: ["Rule", "Global", "Direct"]` and
  `default_mode` = `settings.proxy_mode` capitalized (membership is case-sensitive in sing-box
  `NewServer`; a lowercase entry would pollute the reported `mode-list`). `settings.proxy_mode`
  is the **default mode**: applied at build time, restored after every restart because the config
  is rebuilt on apply.
- **`experimental.cache_file` stays OFF** (locked): `Server.Start()` would restore the cached mode
  and override `default_mode`, breaking mode restoration on restart.
- `route.final` keeps the rule-mode value in all three modes; `clash_mode` rules short-circuit
  before it at match time.
- **Disk `config.json` lags while Running:** the `PATCH` path does not regenerate `config.json`,
  so its baked `default_mode` stays at the pre-switch value until the next apply/restart. The
  on-disk file is **not** authoritative for the running mode; `settings.proxy_mode` is.

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
- Bypass list: at least `localhost`, `127.0.0.1`, `::1`; macOS per-service and Windows `<local>` follow platform conventions
- `restore`: fully write back the backup instead of just "turning the proxy off" (if the user originally had another proxy, restore it)

### 13.2 macOS (implemented, slice 4a)

- Enumerate **enabled** network services via `/usr/sbin/networksetup`, set web / secure web / SOCKS per service
- Bypass: `localhost`, `127.0.0.1`, `::1` (`ice_proxy_sys::BYPASS_COMMON`)
- `backup.extra.services[]`: per-service switch, host, port, bypass list for faithful `restore`
- Failures return `proxy.apply_failed` / `proxy.restore_failed`; **must not** mark `proxy-backup.json` as `applied: true` when apply failed (see `apply_and_record`)
- Real-device gate: `cargo test -p ice-proxy-sys -- --ignored --nocapture` (tag `proxy_sys`)

### 13.3 Windows (implemented, slice 4b)

- Back up and set the user-level WinInet / `Internet Settings` (`ProxyEnable`, `ProxyServer`,
  `ProxyOverride`) under `HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings`;
  other keys (e.g. `AutoConfigURL`) are never touched
- `ProxyServer = 127.0.0.1:mixed_port` covers HTTP/HTTPS; **WinInet's user-level settings expose
  no separate SOCKS field**, so SOCKS is not set on Windows (the mixed inbound still accepts SOCKS
  when an application connects directly)
- `ProxyOverride` = bypass list (`localhost`, `127.0.0.1`, `::1`, `<local>`, `BYPASS_WINDOWS_EXTRA`)
- After `apply` / `restore`, notify via `InternetSetOption` (`INTERNET_OPTION_SETTINGS_CHANGED` /
  `INTERNET_OPTION_REFRESH`)
- `backup.extra` carries the raw tri-state (`proxy_enable` / `proxy_server` / `proxy_override`)
  for **verbatim** restore: a user who had another proxy before gets it back exactly
- Registry writes are per-user (`HKCU`), no elevation required — matches the "no UAC" lock
- The `auto_set_system_proxy` settings gate is removed; the flag is accepted on Windows
- Live gates mirror G4.3/G4.4 (`cargo test -p ice-proxy-sys g4_3 -- --ignored` on Windows);
  temp-hive unit tests run on Windows CI
- No WinTUN / elevated UAC driver in v1

### 13.4 Relationship to "local-only mixed"

The system proxy points at `127.0.0.1:mixed_port`. "Proxy only the browser extension, don't touch the system" is not a v1 main path (a "don't change the system proxy" switch can be added later; `auto_set_system_proxy: false` is already reserved).

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
| `start` | §8.1 |
| `stop` | §8.2 |
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
| `apply_subscriptions` | explicitly rebuild the config (normally auto-Applied internally) |

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
- The three write commands (`set_rule_disabled` / `add_custom_rule` / `remove_custom_rule`) **auto-Apply** after a successful change (hot reload if Running); failures surface via `apply_warning`

---

## 15. Frontend structure (suggested)

```
apps/desktop/src/
├── main.tsx
├── App.tsx                 # layout: status bar + pages
├── api/tauri.ts            # invoke wrapper and types
├── pages/Home.tsx          # start/stop, current node, errors
├── pages/Subscriptions.tsx
├── pages/Logs.tsx
└── pages/Settings.tsx
```

v1 minimal UI set:

- Start/stop buttons disabled per the state machine
- Subscription list: name, format, node count / policy group count / rule count / DNS marker, last update, error, **active switch (single-select)**, parse_warnings, update/delete
- Import input (URL)
- Read-only log tail
- Settings: ports, whether to set the system proxy
- Home: node switching, latency test, active connection count, last 60 seconds traffic chart (Clash API)
- Nodes page: batch latency test and sorting

---

## 16. Logging and observability

- App: `tracing` → `ice-box.log` (`ice_config::init_logging`; append-only without rotation in v1, rotation can come later)
- Core: stdout/stderr → `sing-box.log`
- UI `get_log_view`: merges the two log files and **sorts by time**, each line prefixed `[app]` / `[core]`
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
| `config.empty_outbounds` | no usable nodes |
| `config.invalid` | generation/validation failed |
| `proxy.apply_failed` / `proxy.restore_failed` | system proxy |
| `sub.fetch_failed` | network/timeout |
| `sub.unknown_format` | unrecognized format |
| `sub.parse_failed` | parse failed |
| `sub.empty` | no nodes |

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
| Manual | Start then curl the mixed port; Stop then verify the system proxy is restored |

`configs/examples/` serves as the fixture source.

---

## 21. Locked vs. to-be-documented during implementation

**Locked (do not change silently):** platforms (macOS / Windows), Tauri+React, engine crates, subprocess, system proxy, subscription import/management, sing-box first + Clash compatible, hot reload first.

**Details to write back into this document during implementation:**

- ~~Bundled sing-box **exact version**~~ → **written in §4.3 / this table**: `1.13.19`
- ~~reload HTTP method and path~~ → **written in §9.1**: `PUT /configs?force=true`
- Clash supported protocol checklist → **written in §11.4**: ss / vmess / trojan / socks / http
- ~~final JSON of the minimal DNS config~~ → **written in §12.2**: `type: local` / `final: local`
- ~~Windows port release wait on restart~~ → **written in §9.2**: 500 ms

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