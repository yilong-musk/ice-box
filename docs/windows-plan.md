# Windows Platform Completion Plan

Status: **implemented** (2026-08-24) — slices 4b/4c/5/6/7 landed; what still needs a real Windows host
is the live verification: WinInet G4.3/G4.4 gates, the live sing-box mode-switch gate, and installing
the MSI/NSIS artifacts.
Related: `docs/architecture.md` (§9 hot reload, §13 system proxy, §19 slices, §21 locked).

## 1. Goal

Bring the Windows desktop app to the same functional level as macOS:

1. System proxy actually works (WinInet backend).
2. Mode switching no longer needs a core restart on any platform.
3. Windows is compiled and tested in CI.
4. MSI / NSIS installers build and run.
5. Automated acceptance covers Windows.

**Locked decisions (do not change silently):** sing-box first (no mihomo fork), no TUN / elevated
UAC in v1, system proxy = WinInet user-level Internet Settings, hot reload = restart fallback on
Windows (§9.2), config engine stays platform-free.

## 2. Gap inventory (Windows vs macOS today)

| # | Area | macOS | Windows | Owner |
|---|------|-------|---------|-------|
| 1 | System proxy (slice 4b) | `MacosSystemProxy` implemented + live gates G4.3/G4.4 | `NoopSystemProxy` stub (`create_system_proxy`, `ice-proxy-sys/src/lib.rs:94`) | Slice 4b |
| 2 | Mode switching (Rule/Global/Direct) | SIGHUP hot reload | process restart | Slice 4c |
| 3 | Rule / subscription changes | SIGHUP hot reload | process restart (§9.2, 500 ms port wait) | accepted, no change |
| 4 | CI | Linux + macOS gate | no Windows runner; `cfg(windows)` never compiled | Slice 5 |
| 5 | Installer (G8.2) | `.app` / `.dmg` buildable | MSI/NSIS config exists (`tauri.windows.conf.json`) but never built | Slice 6 |
| 6 | Acceptance | `scripts/run-acceptance-macos.sh` | none | Slice 7 |
| 7 | Process termination | SIGTERM graceful | `child.kill()` hard kill; `taskkill /F` for orphan pids | accepted (note below) |

Accepted notes (no work): Windows process termination stays hard-kill + `taskkill` — there is no
graceful SIGTERM equivalent for sing-box on Windows, and the 500 ms port-release wait in §9.2
already covers the restart path.

## 3. Slices

### Slice 4c — Mode switching via Clash API (chosen approach for hot reload)

**Verified upstream facts (sing-box 1.13.19 source, `experimental/clashapi/`):**

- `PUT /configs` → `updateConfigs` → `render.NoContent` (**204 no-op** — reload through it is impossible).
- `PATCH /configs` with `{"mode": "global"}` → `server.SetMode(mode)` — **real runtime mode switch**:
  validates against `mode-list`, updates `s.mode`, emits the mode hook, clears the DNS cache, persists
  to the cache file when enabled. It is a plain HTTP handler, so it works identically on Windows.
- Routing reacts at match time via the `clash_mode` rule item in `route.rules`.
- `GET /configs` returns `mode` and `mode-list`; `default_mode` + `mode-list` come from the
  `experimental.clash_api` config block.

**Design change:** mode is no longer baked into the config build. The generated config always
carries the full rule set plus two `clash_mode` rules prepended at the top:

```json
{ "route": {
    "rules": [
      { "clash_mode": "global", "outbound": "<global target>" },
      { "clash_mode": "direct", "outbound": "direct" },
      ...subscription rules (unchanged)
    ],
    "final": "<rule-mode final>" } }
```

- `<global target>` = the same outbound that `ProxyMode::Global` routes `final` to today
  (injected `proxy` selector when the profile has no groups, else the top group/fallback), so
  homepage node selection keeps working in global mode.
- Rule mode needs no `clash_mode` rule: nothing matches → normal rules run.
- `experimental.clash_api` gains `mode_list: ["Rule", "Global", "Direct"]` and
  `default_mode: <settings.proxy_mode, capitalized>`; `settings.proxy_mode` becomes the
  **default mode** (applied at build time, restored after every restart because the config is
  rebuilt on apply).
  - **Casing:** emit `default_mode` in the same case as `mode_list`
    (`"rule"→"Rule"`, `"global"→"Global"`, `"direct"→"Direct"`). sing-box `NewServer`
    (`experimental/clashapi/server.go`) checks membership with case-sensitive `common.Contains`;
    a lowercase `"global"` would be prepended as a duplicate mixed-case entry, so `GET /configs`
    would report a polluted `mode-list` (`["global","Rule","Global","Direct"]`).
  - **No `cache_file`:** do not enable `experimental.cache_file` in the generated config.
    `Server.Start()` restores the mode from the cache file when present and overrides
    `default_mode`, breaking mode restoration on restart (runtime `SetMode` persistence to the
    cache is a no-op for the same reason). Lock this.
- DNS block unchanged.

**Code changes:**

1. `crates/ice-config/src/lib.rs` (`build_config`):
   - Stop stripping rules in Global/Direct; always emit the full rule set.
   - Prepend the two `clash_mode` rules as the **first** route rules, ahead of the
     custom-rule expansion, so they precede every custom / subscription rule (a custom `direct`
     rule must never win over `clash_mode: "global"`; the JSON sketch above is the intended order).
   - Add `mode_list` + `default_mode` to the `experimental.clash_api` block.
   - Keep `route.final` at the rule-mode value in all three modes.
   - Update `route_final` logic and the `rule_mode` gate accordingly.
2. `crates/ice-core/src/clash_api.rs`:
   - Add `set_mode(endpoints, mode)` → `PATCH /configs` with `{"mode": ...}`.
   - Add `get_mode(endpoints)` → `GET /configs` (for status display / tests), reusing
     `HealthEndpoints`.
3. `apps/desktop/src-tauri/src/commands.rs` (`set_proxy_mode`) + `orchestrate.rs`:
   - Running: try `set_mode` via Clash API — **no config rebuild, no reload, no restart**.
   - `PATCH` failure (e.g. future sing-box without the endpoint): fall back to the current
     rebuild + reload/restart path (capability gate, fully backward compatible).
   - Stopped: persist settings only (next apply builds with the new `default_mode`).
   - System proxy stays applied in both paths (inbound unchanged).
   - **Disk `config.json` lags while Running:** the PATCH path does not regenerate
     `config.json`, so its baked `default_mode` stays at the pre-switch value until the next
     apply/restart rebuilds it. Harmless — the runtime mode comes from `PATCH` and every apply
     rebuilds the config — but keep the on-disk file as *not* authoritative for the running mode
     and say so in `architecture.md` §12/§13.
   - **PATCH precondition:** `PATCH` only changes routing if the running config carries the
     `clash_mode` rules. After an app upgrade where an old-style core (rules stripped in
     Global/Direct) is still running, the first mode switch must take the rebuild + reload/restart
     fallback once (e.g. always reload on the first PATCH after an upgrade) so the switch is not
     silently ignored.
4. Frontend: no structural change (`Home.tsx` mode buttons and `set_proxy_mode` contract stay);
   optional copy update to drop "切换模式会中断连接" wording.

**Tests:**

- `ice-config`: config in all three modes contains full rules + `clash_mode` rules in the right
  order; route references validate; `default_mode`/`mode_list` present; Global/Direct no longer
  strip rules (update existing `proxy_mode_*` tests).
- `ice-core`: `set_mode` request shape against a mock HTTP server; `get_mode` parse.
- desktop crate: `set_proxy_mode` while Running issues `PATCH` and does not restart (mock core);
  `PATCH` failure falls back to rebuild; while Stopped only persists.
- Live test (real sing-box, `#[ignore]`): start → `PATCH` global → curl through mixed port
  confirms all traffic goes via proxy; `PATCH` direct → curl confirms direct; back to rule.
  Reuse on Windows in Slice 7.

**Docs:** update `architecture.md` §9.1 (reload surface = rule/subscription changes only),
§12/§13 mode notes; update README Status.

### Slice 4b — WinInet system proxy (ice-proxy-sys)

Implement per architecture §13.3:

- `WindowsSystemProxy` (new `crates/ice-proxy-sys/src/windows.rs`):
  - `backup`: read user-level `Internet Settings` (`ProxyEnable`, `ProxyServer`, `ProxyOverride`,
    keep other keys untouched) into `ProxyBackup` (`extra` carries the raw tri-state for faithful
    restore).
  - `apply`: write the three values (`ProxyServer = 127.0.0.1:mixed` covering HTTP/HTTPS, SOCKS
    has no separate user-level field on WinInet — document that), then notify via
    `InternetSetOption(INTERNET_OPTION_SETTINGS_CHANGED | INTERNET_OPTION_REFRESH)`.
  - `restore`: write the backup values back verbatim; `ProxyEnable` state restored exactly
    (user originally had another proxy → it comes back).
  - Bypass: `BYPASS_COMMON` + `<local>` (`BYPASS_WINDOWS_EXTRA` already exists in `bypass.rs`).
- `create_system_proxy()` returns the real backend on Windows.
- Dependency: `winreg` + `windows-sys` (or `windows` crate) — keep it in `ice-proxy-sys` only;
  engine crates stay platform-free (§22 rule 2).
- Registry write is per-user (`HKCU`), no elevation required — matches "no UAC" lock.
- **Remove the Windows settings gate:** drop the `#[cfg(target_os = "windows")]` rejection of
  `auto_set_system_proxy` in `crates/ice-config/src/settings.rs` (`validate`, ~L98-104). It runs
  on both `load_settings` and `save_settings`, so until removed no Windows user can enable the
  system proxy (and any settings file carrying the flag fails to load). After this slice the
  backend is real, so the gate is dead weight; re-enable the flag and let the WinInet path handle
  it. Keep a unit test asserting the flag is accepted on Windows.

**Tests:** unit tests with a temporary registry hive where feasible (`#[ignore]` if not);
live gates mirroring G4.3/G4.4 (`cargo test -p ice-proxy-sys g4_3 -- --ignored` on Windows,
tag `proxy_sys`). Crash-recovery restore path (`recover_if_applied`) is backend-agnostic — verify
it on Windows in acceptance.

### Slice 5 — Windows CI

- `.github/workflows/ci.yml`: add a `windows-latest` job mirroring `gate-macos`:
  - Set `shell: bash` explicitly (windows-latest runners provide Git Bash; `bash scripts/gate.sh`
    must resolve).
  - Fetch sing-box **before** the gate: `./scripts/fetch-singbox.sh win` (installs
    `third_party/sing-box/windows-x86_64/sing-box.exe` from
    `sing-box-1.13.19-windows-amd64.zip`) so binary-dependent lib tests run, matching the macOS
    job.
  - Run `npm run gate` (= `bash scripts/gate.sh`): cargo fmt + clippy + workspace lib tests +
    tsc + vitest (`npm test`) + vite build. Live network proxy tests stay `#[ignore]` (gate must
    not depend on live network).
  - Optional (mirrors macOS): `cargo test -p ice-box --lib 'g9_' -- --nocapture` headless
    acceptance.
- Gate becomes: Linux (existing) + macOS (existing) + Windows (new).

### Slice 6 — Windows packaging (G8.2)

- New script `npm run build:win` (maps to `tauri build --config tauri.windows.conf.json`),
  run on a Windows host (or a Windows CI job).
- `scripts/prepare-singbox-resource.sh` already copies `windows-x86_64/sing-box.exe` when the
  target is win — verify end-to-end; add a PowerShell fallback if Git Bash is unavailable.
- Validate: MSI and NSIS artifacts install; `sing-box.exe` lands in the install dir;
  WebView2 bootstrapper path works.

### Slice 7 — Windows acceptance

- `scripts/run-acceptance-windows.ps1` (or `.sh` under Git Bash) mirroring the macOS script:
  1. `bash scripts/gate.sh`
  2. headless G tests (`cargo test -p ice-box --lib 'g9_'`)
  3. live WinInet proxy roundtrip (G4.3/G4.4 equivalents)
  4. live sing-box start/stop + mode switching (Slice 4c live test)
  5. crash recovery: kill sing-box, relaunch app, proxy restored
- Manual checklist: installer, tray, system proxy on/off from UI, mode switch while downloading.

## 4. Ordering and dependencies

1. **Slice 4c first** — pure win for both platforms, independent of Windows tooling, closes the
   biggest UX gap without waiting for a Windows host.
2. **Slice 4b** — needs a Windows machine for live gates; unit-testable on Linux only if the
   registry layer is abstracted (else gate on Windows CI).
3. **Slice 5** — after 4b/4c so the Windows runner verifies real `cfg(windows)` code.
4. **Slice 6** — after CI passes on Windows.
5. **Slice 7** — final, on a Windows host.

## 5. Acceptance criteria

- [x] Mode switch while Running: no core restart, no connection drop, no system proxy churn
      (both platforms; automated live test `g9_11_live_mode_switch_via_clash_api`, `#[ignore]`)
- [~] Windows: apply sets WinInet proxy + notifies; restore returns the user's previous settings
      exactly (live G4.3/G4.4 analogues in `ice-proxy-sys` — must run on a Windows host)
- [x] Windows runner green in CI (fmt, clippy, tests, tsc, vitest) — `gate-windows` job added;
      `cargo check/clippy --target x86_64-pc-windows-gnu` verified locally
- [~] MSI and NSIS install on a clean Windows host and launch with bundled `sing-box.exe`
      (`npm run build:win` — must run on a Windows host)
- [~] `run-acceptance-windows` fully green (script added — must run on a Windows host)

## 6. Out of scope

- TUN / WinTUN / elevated drivers.
- mihomo or any non-sing-box core.
- In-process reload on Windows (blocked upstream; Slice 4c is the mitigation).
- Graceful Windows process termination (hard kill accepted).
- macOS behavior regression: Slice 4c changes macOS mode switching from SIGHUP to `PATCH /configs`
  — same user-visible result, better (no reload at all).