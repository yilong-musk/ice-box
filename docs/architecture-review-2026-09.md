# Architecture and Implementation Review — September 2026

Status: **Phase 0–4 are done on this branch (unreleased).** Every item has a stable ID
(`SEC-1`, `CORE-3`, …) so it can be referenced from issues, PRs and the
CHANGELOG. When an item is fixed, mark it `Done (vX.Y.Z)` in the summary
table rather than deleting it, so the review stays auditable.

Companion documents: `architecture.md` (v1 spec), `tun.md` (TUN capture spec),
`release-process.md`. File references below are `path:line` against commit
state on 2026-09-07 (`main`); line numbers drift, symbol names do not.

---

## 1. Scope and method

The whole workspace was reviewed: `apps/desktop` (Tauri shell + React UI),
`crates/*` (core lifecycle, config engine, subscription, system proxy, TUN,
privileged helper/launcher), scripts, CI/release workflows and `docs/`.

Method: direct reading of the orchestration and privileged paths
(`orchestrate.rs`, `capture.rs`, `ice-core`, `ice-helper`, `ice-tun-launcher`,
`ice-tun-sys/coordinator.rs`), parallel module sweeps for the rest, then
source-level re-verification of every High finding before it was written down.
Claims that could not be verified against the pinned sing-box binary are
marked **needs verification**.

Overall judgement: the architecture in `docs/architecture.md` is sound and the
layering intent (UI knows no protocols, thin Tauri shell, sing-box as a
subprocess, layered config, rollback on failure) is largely respected. The
state machines, TUN journal and fail-closed recovery are carefully done, and
the mock seams (`ProcessSpawner`, `HealthProbe`, `ConfigReloader`,
`TunBackend`, `SystemProxy`) give unit tests good reach. The problems are
concentrated in three places: the **privileged boundary** trusts input it
should not, **CI does not run the integration tests that cover the riskiest
code**, and the code has **drifted from its own spec** in several documented
places.

## 2. Severity legend

| Level | Meaning |
|---|---|
| **High** | Exploitable privilege boundary, data loss, or a safety property the spec promises that the code does not deliver. Fix before the next formal release. |
| **Medium** | Wrong behaviour reachable in normal use, or a structural problem that will keep producing bugs. Fix in the next one or two releases. |
| **Low** | Quality, performance, drift, hygiene. Fix opportunistically or bundle into a refactor. |

## 3. Summary table

| ID | Sev | Area | Title | Status |
|---|---|---|---|---|
| SEC-1 | High | Privileged core | Elevated sing-box loads a user-writable config whose content is never validated | Done (unreleased) |
| SEC-2 | High | Windows TUN | Launcher/core live in a per-user-writable dir; task XML imported from user dir; `Command` not verified after import | Done (unreleased) |
| SEC-3 | High | Windows TUN | `TaskCoreCoordinator::start_with_config` discards `config_path` | Done (unreleased) |
| SEC-4 | High | macOS helper | `SetDns` executes without validating `service`/`servers` | Done (unreleased) |
| SEC-5 | Medium | macOS helper | Helper token file `0644`, plist `0644`, socket `0666` | Done (unreleased) |
| SEC-6 | Medium | ice-core | `adopt_external` adopts any pid without identity check | Done (unreleased) |
| SEC-7 | Low | macOS helper | `constant_time_eq` early-returns on length mismatch | Done (unreleased) |
| SEC-8 | Low | ice-config | `force_tun_gate_ready` is a `pub`, process-global, irreversible test hook | Done (unreleased) |
| CORE-1 | Medium | ice-core | `stop()` errors while `Stopping` (spec says stop is idempotent) | Done (unreleased) |
| CORE-2 | Medium | ice-core | Health check is TCP-connect only | Done (unreleased) |
| CORE-3 | Medium | ice-core | `PidProcess::try_wait` uses `kill(pid, 0)`; zombies look alive | Done (unreleased) |
| CORE-4 | Medium | ice-core | Windows `pid_is_alive` treats `ACCESS_DENIED` as dead; `PidProcess` treats it as alive | Done (unreleased) |
| CORE-5 | Medium | ice-core | `TrafficMonitor::ensure_thread` panics on poisoned slot and never recovers a dead supervisor | Done (unreleased) |
| CORE-6 | Low | ice-core | `reclaim_orphan_cores_with_config` is a no-op on Windows | Done (unreleased) |
| CORE-7 | Medium | ice-core / config | Neither `sing-box.log` nor `ice-box.log` rotates; core log level is `info` | Done (unreleased) |
| CORE-8 | Low | ice-core | Clash API failures are reported as `CoreError::SpawnFailed` | Done (unreleased) |
| ORCH-1 | Medium | src-tauri | `state.core` mutex is held for multi-second transitions; status polling blocks behind it | Done (unreleased) |
| ORCH-2 | Low | src-tauri | Error classification by message substring (`"lock poisoned"`) | Done (unreleased) |
| ORCH-3 | Medium | src-tauri / UI | `save_settings` takes whole-snapshot writes from two pages; last writer wins | Done (unreleased) |
| TUN-1 | High | ice-tun-sys (macOS) | `verify` treats a failed DNS probe as "consistent" | Done (unreleased) |
| TUN-2 | Medium | ice-tun-sys | Journal write errors ignored in recovery | Done (unreleased) |
| TUN-3 | Low | ice-config | `tun_network_cidr` accepts prefix > 32 / > 128 | Done (unreleased) |
| CFG-1 | Medium | ice-config | `validate_config` only checks that `inbounds`/`outbounds` keys exist | Done (unreleased) |
| CFG-2 | Medium | ice-config | Invalid `settings.json` makes `load_settings` fail hard; no fallback | Done (unreleased) |
| CFG-3 | Low | ice-config | `restore_runtime_config_from_bak` uses non-atomic `fs::copy` | Done (unreleased) |
| SUB-1 | High | ice-subscription | Profile commit is `remove_dir_all` then `rename`; crash in between loses the subscription | Done (unreleased) |
| SUB-2 | Medium | ice-subscription | Clash >500 nodes is a hard error (spec: truncate + warn); sing-box JSON path has no caps | Done (unreleased) |
| SUB-3 | Medium | ice-subscription | Custom HTTP/1.1 + rustls: webpki roots only, `ClientConfig` rebuilt per request, no gzip | Done (unreleased) |
| SUB-4 | Medium | ice-subscription | Fetch worker `.expect()`s on a poisoned queue; a panic kills auto-update permanently | Done (unreleased) |
| SUB-5 | Low | ice-subscription | `NoActiveSubscription` / `Io` mapped to `sub.fetch_failed` | Done (unreleased) |
| SUB-6 | Low | ice-subscription | `PROFILE_LOAD_CACHE` is a process-global static and clones the whole profile on every hit | Done (unreleased) |
| PROXY-1 | Medium | ice-proxy-sys | Corrupt `proxy-backup.json` reads as "not applied"; `recover_if_applied` errors instead — inconsistent, proxy may leak | Done (unreleased) |
| PROXY-2 | Low | ice-proxy-sys (macOS) | Live probe = full `networksetup` backup (subprocess storm on cache miss) | Done (unreleased) |
| ARCH-1 | Medium | workspace | `ice-config` is a shared kernel with platform `cfg`s; `ice-engine` façade is unused | Done (unreleased) |
| ARCH-2 | Low | workspace | `ice-tun-sys` (lib) depends on `ice-tun-launcher` (bin crate) | Done (unreleased) |
| ARCH-3 | Medium | workspace | Three error-code systems; ~39 string-literal `tun.*` codes | Done (unreleased) |
| ARCH-4 | Low | workspace | Oversized files; tests inlined into production files | Done (unreleased) |
| ARCH-5 | Low | ice-tun-sys | Four `start_with_config` implementations in `coordinator.rs` | Done (unreleased) |
| PERF-1 | Low | ice-config | Deep clones and per-rule file stats on every Apply | Done (unreleased) |
| PERF-2 | Medium | UI | Uncoordinated polling from four places; keeps running while hidden in tray | Done (unreleased) |
| PERF-3 | Low | UI | Traffic chart refetches the full 120-point window every second | Done (unreleased) |
| FE-1 | Medium | UI | No shared server-state layer; stale-response guard applied inconsistently | Done (unreleased) |
| FE-2 | Low | UI | `Settings.tsx` ~1000 lines; reads status once, misses TUN transition states | Done (unreleased) |
| FE-3 | Low | UI | `useCallback(fn, [])` over props that change every render (`Nodes.tsx`) | Done (unreleased) |
| FE-4 | Low | UI | Unused `@tanstack/react-virtual`; `tailwindcss` in `dependencies` | Done (unreleased) |
| FE-5 | Low | UI | i18n gaps: hardcoded punctuation/labels, raw enum display, English backend text in zh UI | Done (unreleased) |
| FE-6 | Low | UI tests | Tests assert on zh copy and classNames | Done (unreleased) |
| FE-7 | Low | website | `browser-api.ts` mirrors `api/tauri.ts` by hand | Done (unreleased) |
| CI-1 | High | CI | `cargo test --lib` never runs `crates/*/tests/*.rs` (~90 TUN integration tests) | Done (unreleased) |
| CI-2 | Medium | CI | No `cargo audit` / `cargo deny` / `npm audit` | Done (unreleased) |
| CI-3 | Low | build | No `rust-toolchain.toml`; no `[profile.release]` | Done (unreleased) |
| CI-4 | Low | CI | Action versions inconsistent across workflows; no `concurrency` on ci/release | Done (unreleased) |
| CI-5 | High | packaging | `libcronet.dll` copied to `resources/` but not declared in `bundle.resources` | Done (unreleased) |
| CI-6 | Low | repo | GeoIP rule-sets committed twice (30 + 30 files) | Done (unreleased) |
| CI-7 | Low | release | Root `LICENSE` not uploaded although `release-process.md` says it is | Done (unreleased) |
| CI-8 | Low | scripts | `fetch-geoip.sh` downloads without checksum verification | Done (unreleased) |
| CI-9 | Low | CI | Windows job skips `ice-proxy-sys` tests; local gate excludes `ice-box` | Done (unreleased) |
| DOC-1…9 | Low | docs | Spec/implementation drift (see §4.12) | Done (unreleased) |

---

## 4. Findings and fixes

Each finding lists **Where**, **Problem**, **Impact**, **Fix** and **Verify**
(the acceptance criterion a PR must meet).

### 4.1 Security — privileged surfaces

#### SEC-1 (High) Elevated core loads a user-writable, content-unvalidated config

**Where**

- macOS helper start path: `crates/ice-helper/src/lib.rs:261-264` — only
  `validate_config_path` (location inside the data dir) is checked before
  `runner.start(core_bin, canonical, core_log)` runs sing-box as root.
- Windows: the scheduled task runs `ice-tun-launcher.exe` elevated with the
  data dir baked into `Arguments`; the launcher reads `config.json` from that
  user-writable directory (`crates/ice-tun-launcher/src/main.rs`).
- Subscription normalisation passes outbound objects through verbatim:
  `crates/ice-subscription/src/lib.rs:1378-1396` (`outbound: item.clone()`),
  and `route.rule_set` / `dns` are `.cloned()` into the profile.
- `ice-config::validate_config` (`crates/ice-config/src/lib.rs:998-1009`)
  does not look at content either (see CFG-1) and in any case runs in the
  unprivileged app, outside the helper's trust boundary.

**Problem**
The privileged component trusts whatever JSON the unprivileged app (or
anything running as the same user) puts in `config.json`. Remote subscription
servers control large parts of that JSON. sing-box config can reference files
and executables — e.g. the `tor` outbound's `executable_path` /
`data_directory`, `*_path` keys on TLS/certificate/rule-set/cache objects,
`log.output`, `experimental.clash_api.external_ui`. Which of these are
compiled into the pinned 1.13.19 build **needs verification**, but a zero-filter
boundary is the defect regardless of the exact gadget.

**Impact**
(a) A malicious or compromised subscription server gets its JSON loaded by a
root (macOS) or Administrator/SYSTEM (Windows) sing-box.
(b) Any same-user process can rewrite `config.json` and ask the helper (unix
socket, `0666`, uid-checked) or the scheduled task (`schtasks /Run`) to start
it → local privilege escalation without a UAC/authorization prompt after the
one-time setup. This is the classic "privileged helper trusts its client" LPE.

**Fix**

1. Add a small, dependency-light crate `crates/ice-config-guard` (serde_json
   only) exposing
   `pub fn sanitize_for_elevated_core(cfg: &mut serde_json::Value, ctx: &GuardContext) -> Result<(), GuardError>`
   with `GuardContext { data_dir, resources_dir, allowed_clash_listen: loopback only }`.
   Rules (deny by default):
   - `inbounds[].type` ∈ {`tun`, `mixed`}; reject unknown keys per type.
   - `outbounds[].type` ∈ an explicit allowlist (`direct`, `block`, `dns`,
     `selector`, `urltest`, `shadowsocks`, `vmess`, `vless`, `trojan`,
     `hysteria`, `hysteria2`, `tuic`, `http`, `socks`, `anytls`, `wireguard`
     without `system`/`interface_name`). `tor`, `ssh` with key paths and any
     type not in the list are rejected.
   - Reject any key named `*_path`, `path`, `executable_path`,
     `data_directory`, `output`, `external_ui`, `external_ui_download_url`
     anywhere, except `route.rule_set[].path` which must canonicalise under
     `resources_dir`.
   - Force-overwrite `log` (`level`, `output` owned by the helper) and
     `experimental.cache_file.path`; require `experimental.clash_api.external_controller`
     to be loopback.
   - Size caps: total bytes, outbound count, rule count.
2. Call it in `ice-helper` `HelperCommand::Start` (parse the file, sanitise,
   write the sanitised copy to a **root-owned** path, start from that copy —
   never from the user-writable file) and in `ice-tun-launcher` before spawn
   (write to `%ProgramData%\ice-box\run\config.json` with admin-only ACL).
3. Apply the same allowlist in `ice-subscription` normalisation so the
   user-mode core benefits too and users see rejected outbounds as warnings.
4. New error code `tun.config_rejected` with the offending JSON pointer in the
   message; surface it in the UI.

**Verify**
- Golden test: output of `build_runtime_config` for every fixture passes the
  guard unchanged.
- Negative fixtures: `tor` outbound, `certificate_path`, non-loopback
  `external_controller`, `rule_set.path` outside resources → rejected with the
  right pointer.
- `helper_e2e.rs` gains a rejected-config case; the test runs in CI (CI-1).
- Manual: edit `config.json` to include a `tor` outbound → helper refuses,
  UI shows `tun.config_rejected`.

#### SEC-2 (High) Windows TUN elevation: writable binaries, XML TOCTOU, unverified `Command`

**Where**

- Pin check in the launcher: `crates/ice-tun-launcher/src/main.rs:252-267`
  (`verify_pinned_core`) hashes `current_exe()` and `sing-box.exe` against the
  pin stored in the task XML.
- App-side check: `crates/ice-tun-sys/src/coordinator.rs:909-957`
  (`tun_task_pin_matches` → `verify_task_binaries`) compares the pin with the
  launcher/core files at the path **the app passes in**.
- Task XML is written to the user-writable data dir and then handed to the
  elevated import: `apps/desktop/src-tauri/src/commands.rs:1010-1011`.
- `command_matches_launcher` (`crates/ice-tun-launcher/src/lib.rs`) exists
  but is only referenced from tests.

**Problem**

1. The launcher and core live in the per-user install directory (the crate's
   own comment says so). A replaced `ice-tun-launcher.exe` does not run
   `verify_pinned_core`, and `schtasks /Run /TN ice-box-tun` does not check
   anything → arbitrary code as Administrator with no UAC prompt.
2. The XML file is written by the unprivileged app and imported by the
   elevated process; anything running as the user can swap it in between.
3. After import, only the pin (in `Description`) is verified. A swapped XML
   that keeps the genuine pin but changes `Exec/Command` passes
   `tun_task_pin_matches`.

**Impact** UAC bypass / persistence for any same-user code; silent
substitution of what the elevated task actually runs.

**Fix**

1. At elevation time, the elevated step copies `ice-tun-launcher.exe`,
   `sing-box.exe`, `wintun.dll` and `libcronet.dll` to
   `%ProgramData%\ice-box\bin\` with an explicit ACL (SYSTEM + Administrators:
   full; Users: read/execute) and points the task `Command` there. The pin is
   computed over the protected copies. The launcher additionally refuses to
   run if `current_exe()` is not under the protected directory.
2. Do not import an XML from the user's data dir. Make the elevated step a
   launcher sub-command (`ice-tun-launcher --install --data-dir <dir>`) that
   renders the XML in memory (reuse `render_tun_task_xml`) into a temp file
   under `%ProgramData%\ice-box\` with admin-only ACL and imports it (or use
   the Task Scheduler COM API and skip the file entirely).
3. Extend `verify_task_binaries` to also check `Exec/Command` and
   `Exec/Arguments` against the expected protected launcher path and data dir
   using the existing `command_matches_launcher`; a mismatch is
   `TunErrorCode::PermissionRequired` with "re-run elevation setup".
4. On uninstall/upgrade: re-pin (already handled by `ensure_tun_elevation`)
   and remove stale protected copies.

**Verify**
- Unit: `verify_task_binaries` rejects an XML whose `Command` differs while
  the pin matches (fixture).
- Manual on Windows: replace the launcher in the user install dir → task
  still runs the protected copy and the app reports the pin mismatch; tamper
  `Command` via an exported/edited XML → `tun_task_pin_matches` is false.
- `windows_backend.rs` integration tests run in CI (CI-1).

#### SEC-3 (High) `TaskCoreCoordinator::start_with_config` discards the config path

**Where** `crates/ice-tun-sys/src/coordinator.rs:1092-1093`
(`let _ = config_path;`). The doc comment claims the path is verified against
the one baked into the task.

**Problem / Impact** The app believes it started the config it just wrote;
the task starts whatever path its `Arguments` say. Combined with SEC-2 the
gap is silent.

**Fix** Read the task XML, extract `--data-dir` / config path from
`Arguments`, canonicalise both sides, and fail with
`TunErrorCode::ApplyFailed("task pinned to a different config path; re-run ensure_tun_elevation")`
on mismatch. Add `extract_tun_task_args_from_xml` next to
`extract_tun_task_pin_from_xml`.

**Verify** Unit test with a fake XML where the path differs → `Err`; matching
path → `Ok(pid)`.

#### SEC-4 (High) Helper `SetDns` runs without input validation

**Where** `crates/ice-helper/src/lib.rs:266-269` calls
`set_system_dns(service, servers)` directly. The validators
`validate_dns_service` / `validate_dns_server`
(`crates/ice-tun-sys/src/helper_protocol.rs:65,81`) are only called from
tests. `docs/tun.md:90` still says the helper only starts and stops the core.

**Problem / Impact** Root-level `networksetup -setdnsservers <service> <servers…>`
with client-controlled strings. Even if `networksetup` does not interpret
shell metacharacters, unvalidated service names and non-IP "servers" are a
root-level footgun and violate the protocol's documented contract.

**Fix** In the daemon: `validate_dns_service(service)?;` then
`for s in servers { validate_dns_server(s)?; }`; cap `servers.len() <= 4`
and reject empty. Return a distinct `HelperResponse` error code
(`invalid_argument`). Update `docs/tun.md` (DOC-3).

**Verify** Daemon unit tests: `"Wi-Fi; rm -rf /"`, `"8.8.8.8 evil"`, 5
servers → rejected; `"Wi-Fi", ["1.1.1.1"]` → accepted.

#### SEC-5 (Medium) Token file `0644`, plist `0644`, socket `0666`

**Where** `crates/ice-helper/src/install.rs:241` (`set_mode(&token_file, 0o644)`),
`crates/ice-helper/src/main.rs:134` (socket `0o666`).

**Problem** The shared secret is world-readable. Cross-user abuse is currently
blocked by the daemon's `getpeereid` uid check, so this is defence-in-depth,
but a secret should never be readable by everyone.

**Fix** Write the token `0600`, owned by the installing uid (daemon runs as
root and can still read it). Keep the socket `0666` (needed for connect) and
document explicitly in `tun.md` that **uid check is the primary control and
the token is secondary**. Plist can stay `0644` (launchd requires readable).

**Verify** `stat -f %Lp` on the token after install == `600`; helper e2e still
passes.

#### SEC-6 (Medium) `adopt_external` adopts any pid

**Where** `crates/ice-core/src/lib.rs:227-238`. `reclaim_orphan_pid` uses
`looks_like_singbox_process`; `adopt_external` does not.

**Impact** On pid reuse the controller "owns" an unrelated process and will
later `SIGTERM`/`SIGKILL` it (`force_kill_pid`, `lib.rs:1046-1056`).

**Fix** Require `looks_like_singbox_process(pid)` (and, where available,
match the process's executable path to our bundled/protected core path)
before adopting; otherwise return a new `CoreError::AdoptRejected(pid)`.
Record process start time at adopt and re-check it before any kill.

**Verify** Unit test adopting the test process's own pid → `Err`.

**Done (unreleased)** `adopt_external` requires `looks_like_singbox_process`
and matches the live image against `CorePaths::adopt_binaries()` (bundled
plus protected copies). Empty on-disk candidates fall back to the name
check. Tests cover rejecting this process and adopting a live `sing-box`-named
stub (Unix).

#### SEC-7 (Low) `constant_time_eq` length early-return

**Where** `crates/ice-helper/src/lib.rs:136-146`.

**Fix** Compare fixed-length digests (`sha256(token)`) with
`subtle::ConstantTimeEq`, or pad. Low value since token length is fixed, but
cheap.

**Done (unreleased)** Both sides are SHA-256 hashed, then the 32-byte digests
are compared with `subtle::ConstantTimeEq` (`crates/ice-helper/src/lib.rs`).

#### SEC-8 (Low) Test hook exposed in the production API

**Where** `crates/ice-config/src/lib.rs:86-91` — `pub fn force_tun_gate_ready()`
sets a process-global, irreversible `OnceLock`.

**Fix** Gate behind `#[cfg(any(test, feature = "test-hooks"))]`; enable the
feature from `apps/desktop/src-tauri` `dev-dependencies` only. Same treatment
for any other test-only `pub` items found by
`rg "Test-only" crates/ --type rust`.

**Verify** `cargo build --release -p ice-box` has no `force_tun_gate_ready`
symbol; tests still compile.

### 4.2 Core lifecycle (`ice-core`)

#### CORE-1 (Medium) `stop()` rejects while `Stopping`

**Where** `crates/ice-core/src/lib.rs:316-320`. `architecture.md` §8.2 says
stop is best-effort idempotent.

**Fix** Return `Ok(())` when already `Stopping`/`Stopped` (optionally wait for
the in-flight stop to finish, bounded by the existing stop timeout). Keep
rejecting only for states where stop is meaningless (`Starting` with no pid).

**Verify** Existing state-machine tests plus `stop(); stop()` →
`Ok, Ok`.

#### CORE-2 (Medium) Health check is TCP-connect only

**Where** `HealthProbe` implementation and `wait_health`
(`crates/ice-core/src/lib.rs`, around `reload` at 369-390).

**Problem** After `SIGHUP` sing-box tears down and rebuilds listeners. A TCP
connect succeeding proves only that *some* process owns the port. Also,
production and test probes have diverged semantically.

**Fix** Extend `HealthProbe` with `fn probe_http(&self, ep) -> Result<()>`
that performs `GET /version` on the Clash API with the secret and requires
2xx; `wait_health` requires connect **and** HTTP for the Clash endpoint. On
reload also assert the pid is unchanged. Keep the fake probe in tests honest by
making it implement both.

**Verify** Reload tests with a fake that answers connect but not HTTP →
reload reported failed → restart path taken.

#### CORE-3 (Medium) `PidProcess::try_wait` uses `kill(pid, 0)`

**Where** `crates/ice-core/src/process.rs:350`.

**Problem** A zombie (adopted, un-reaped pid) answers `kill(pid,0)` with
success → "survived TERM and KILL" errors and a stuck `Stopping`.

**Fix** Linux: read `/proc/<pid>/stat` state (`Z` ⇒ exited). macOS:
`proc_pidinfo(PROC_PIDTBSDINFO)` / `sysctl KERN_PROC_PID` and check
`SZOMB`. Treat zombie as exited.

**Verify** Unit test that forks a child, lets it exit without reaping, and
checks `try_wait` reports exit.

#### CORE-4 (Medium) Windows liveness semantics disagree

**Where** `crates/ice-core/src/lib.rs:977-990` (`pid_is_alive`: any
`OpenProcess` failure ⇒ dead) vs `crates/ice-core/src/process.rs:374-381`
(`try_wait`: `ERROR_ACCESS_DENIED` ⇒ alive). Unix `pid_is_alive` already
treats `EPERM` as alive (`lib.rs:968-975`).

**Impact** Startup reclamation may consider an elevated core dead and clear
its pid file while it is still running.

**Fix** In `pid_is_alive` on Windows, after a null handle check
`GetLastError() == ERROR_ACCESS_DENIED` ⇒ `true`. Share one helper between
`pid_is_alive` and `try_wait`.

**Verify** Windows-only test: `pid_is_alive(4)` (System) is `true`.

#### CORE-5 (Medium) `TrafficMonitor::ensure_thread` cannot recover

**Where** `crates/ice-core/src/traffic.rs:121-132` — `.expect()` on the
slot mutex; the supervisor slot is never cleared if the thread panics.

**Fix** Wrap the supervisor body in `catch_unwind`; on exit (normal or panic)
clear the slot and log; `ensure_thread` uses `lock().unwrap_or_else(PoisonError::into_inner)`
and re-spawns with exponential backoff (cap 30 s).

**Verify** Test with an injected panicking stream: monitor recovers on the
next `ensure_thread`.

#### CORE-6 (Low) Orphan reclamation is a no-op on Windows

**Where** `crates/ice-core/src/lib.rs:789-793` returns `0` under
`cfg(not(unix))`.

**Fix** Enumerate processes (`CreateToolhelp32Snapshot`), keep those whose
image path (`QueryFullProcessImageNameW`) is our bundled or protected
`sing-box.exe`, and terminate the ones whose command line references our
config path (command line via `NtQueryInformationProcess`, or fall back to
image-path match only). Low priority; document the limitation until done.

**Done (unreleased)** Windows enumerates with `CreateToolhelp32Snapshot` and
reads the command line (`ProcessCommandLineInformation`), then applies the
same `sing-box run -c <config>` matcher as Unix.

#### CORE-7 (Medium) Logs never rotate; core logs at `info`

**Where** `crates/ice-core/src/process.rs:102-106` opens `sing-box.log` in
append mode; generated config sets `"log": {"level": "info"}`
(`crates/ice-config/src/lib.rs:253,619`); `ice-box.log` uses a plain file
appender.

**Impact** Per-connection info logging with no rotation fills the disk on
long-running machines — the most likely operational incident in the current
code.

**Fix**
- App log: `tracing-appender` rolling (daily or size-based, keep 5).
- Core log: default `log.level` to `warn` with a settings toggle for `info`
  (debug sessions); on every core start rotate `sing-box.log` →
  `sing-box.log.1..3` when above 20 MB; `core_watch` checks size each tick and
  raises a `logs.oversized` diagnostic when the running core exceeds the cap
  (rotation while running requires a restart — offer it via a "Clear logs"
  action in the Logs page).

**Verify** Unit test for the rotation helper; manual: fill the log past the
cap and observe rotation on restart.

#### CORE-8 (Low) Clash API errors surface as `SpawnFailed`

**Where** `crates/ice-core/src/clash_api.rs:27,67`.

**Fix** Add `CoreError::ClashApi { path, source }` mapped to a new
`ErrorCode::CoreApiFailed` (`core.api_failed`).

### 4.3 Orchestration and locking (`apps/desktop/src-tauri`)

#### ORCH-1 (Medium) `state.core` held across multi-second operations

**Where** `apps/desktop/src-tauri/src/commands.rs:581-585` (`enable_tun`
under the `core` lock: stop core → wait for port release → elevated start →
health check), `save_settings` holds `core` and `proxy` through
`apply_after_change` (reload + up to 5 s health + possible restart), while
`collect_status` (`commands.rs:424-426`) needs `core.lock()` for a snapshot.

**Impact** Every UI status poll (2 s) queues behind a transition; the UI
looks frozen exactly when the user wants feedback.

**Fix** Split state from control: the controller publishes an immutable
`CoreSnapshot` (status, pid, mode, last error, generation) into
`Arc<ArcSwap<CoreSnapshot>>` (or `RwLock`) on every transition;
`collect_status` reads the snapshot lock-free and never takes `core`. Emit a
Tauri event `core://status-changed` on publish so the UI can react instead of
polling (see PERF-2).

**Verify** Test: hold `core` in one thread for 3 s while `collect_status`
returns within 50 ms in another.

**Done (unreleased)** `CoreSnapshotHub` publishes an immutable snapshot;
`collect_status` never takes `core`. The shell emits `core://status-changed`
on publish (`apps/desktop/src-tauri/src/core_snapshot.rs`).
`collect_status_does_not_block_on_core_lock` covers the verify case.

#### ORCH-2 (Low) Error classification by message substring

**Where** `apps/desktop/src-tauri/src/shutdown.rs:136-139`.

**Fix** Add `ErrorCode::LockPoisoned` (`app.lock_poisoned`), have
`lock_poisoned()` use it, and match on `err.code`. Part of ARCH-3.

#### ORCH-3 (Medium) Whole-snapshot `save_settings` from two pages

**Where** Home and Settings both call `saveSettings(fullSnapshot)`; the
backend only preserves `proxy_service_enabled`
(`commands.rs:71 retain_proxy_service_enabled`).

**Fix** Either (a) a `SettingsPatch` (all fields `Option`) applied server-side
with `Option::take` semantics, or (b) add `revision: u64` to `AppSettings`
returned by `get_settings`, require it on save, reject stale writes with
`settings.stale` and have the UI reload and retry. (a) is simpler and removes
the race class entirely.

**Verify** Concurrent save from two callers changing different fields → both
persist.

**Done (unreleased)** `save_settings` takes `SettingsPatch`; omitted fields
keep the on-disk value. Home and Settings send partial patches so they cannot
clobber each other. `proxy_service_enabled` is optional on the patch type for
Home start/stop; the Settings form still omits it.

### 4.4 TUN / capture

#### TUN-1 (High) macOS `verify` treats a failed DNS probe as consistent

**Where** `crates/ice-tun-sys/src/macos.rs:849-875` —
`dns_consistent = … .unwrap_or(true)` while `dns_owned` in the same function
uses `unwrap_or(false)`.

**Impact** With DNS state unknown the backend can mark the transition
`enabled` and later recovery may leave the machine's DNS pointing at a dead
resolver.

**Fix** Make the probe result tri-state (`Option<bool>`); `None` is
"unknown": keep verifying up to the existing deadline, then fail the
transition with `tun.healthcheck_failed` (fail-closed, consistent with the
journal design). Log the probe error.

**Verify** `macos_backend.rs` gains a fault-injected DNS probe failure case;
runs in CI (CI-1).

**Done (unreleased)** `verify` treats a failed DNS probe as unknown: not
consistent and still owned. Apply keeps probing until the adapter-appear
deadline, then fails closed with `tun.healthcheck_failed`.

#### TUN-2 (Medium) Journal record errors ignored in recovery

**Where** `crates/ice-tun-sys/src/recovery.rs:107` (`let _ = journal.record(…)`).

**Fix** Propagate; if the journal cannot be written recovery must return
`RecoveryRequired` (fail-closed) rather than continue with an unrecorded
step. At minimum `tracing::error!`.

#### TUN-3 (Low) `tun_network_cidr` accepts an out-of-range prefix

**Where** `crates/ice-config/src/lib.rs:771-792` — `32 - prefix` /
`128 - prefix` with `prefix: u8` unchecked (`/40` underflows; debug panic,
wrong mask in release).

**Fix** Return `None` for `prefix > 32` (v4) / `> 128` (v6) and validate
`tun.address` in `AppSettings::validate`.

**Verify** Unit tests for `/0`, `/32`, `/33`, `/128`, `/129`.

### 4.5 Config and subscription

#### CFG-1 (Medium) `validate_config` is a key-existence check

**Where** `crates/ice-config/src/lib.rs:998-1009`. `architecture.md:586`
promises "non-empty inbounds/outbounds".

**Fix** Validate: arrays non-empty; tags unique across inbounds and across
outbounds; `route.final` and every `route.rules[].outbound` reference an
existing outbound; selector/urltest `outbounds` members exist; `experimental.clash_api.external_controller`
is loopback unless `allow_lan`. Return `ConfigError::Invalid(String)` with
the JSON pointer.

**Verify** Negative fixtures for each rule; all existing generated configs
pass.

#### CFG-2 (Medium) Invalid `settings.json` fails hard

**Where** `crates/ice-config/src/settings.rs` (`AppSettings::validate` →
`load_settings` error).

**Fix** On parse/validation failure: rename the file to
`settings.json.invalid-<timestamp>`, load defaults, and return a
`Diagnostic { code: "settings.reset", reason }` that the UI shows once.

**Verify** Corrupt file → app starts with defaults and the banner appears.

#### CFG-3 (Low) Non-atomic backup restore

**Where** `crates/ice-config/src/lib.rs:1089-1099` (`fs::copy`).

**Fix** Reuse the atomic write helper (temp + fsync + rename) used for
`config.json`.

#### SUB-1 (High) Profile commit deletes before renaming

**Where** `crates/ice-subscription/src/store.rs:113-118`
(`remove_dir_all(&final_dir)` then `rename(staging, final_dir)`).

**Impact** A crash or power loss between the two calls leaves no profile at
all; the user's only subscription disappears.

**Fix** `rename(final, final.old-<ts>)` → `rename(staging, final)` →
`remove_dir_all(final.old-*)`; on startup sweep leftovers, and if `final` is
missing but an `.old-*` exists, restore it. (Alternative: revisioned
directories plus an atomically-written `current` pointer file.)

**Verify** Fault-injection test that aborts between the renames and checks a
profile still loads.

**Done (unreleased)** Commit is `rename(final, .old-*)` then
`rename(staging, final)` under a process lock; startup restores `.old-*`
when `final` is missing. Tests cover leftover `.old` and an abort window
where staging still exists.

#### SUB-2 (Medium) Node/rule caps inconsistent with spec

**Where** `crates/ice-subscription/src/clash/proxies.rs:33-35` returns
`SkipReason::TooMany` above 500; the sing-box JSON path has no caps.
`architecture.md:494` says "truncate + warning" and lists 10k rules / 128
groups / 500 nodes.

**Fix** One `Limits` struct applied by both parsers: truncate to the cap,
record `warnings.push(Truncated { kind, dropped })` in the subscription
summary, never hard-fail on size.

**Verify** Fixtures with 501 nodes in both formats → 500 kept + warning.

#### SUB-3 (Medium) Custom TLS fetch path

**Where** `crates/ice-subscription/src/tls_fetch.rs:65-67` builds
`RootCertStore` + `ClientConfig` per request with webpki roots only; the
hand-rolled HTTP/1.1 client does not handle `Content-Encoding: gzip`.
`architecture.md:842` claims "reqwest / system roots".

**Fix** Use `rustls-platform-verifier` (system trust store — corporate/self-
signed CAs then work) and cache `Arc<ClientConfig>` in a `OnceLock`; send
`Accept-Encoding: identity` explicitly (or add `flate2` and decode gzip).
Fix DOC-4.

**Verify** Fetch against a server using a locally-trusted private CA
succeeds; a gzip-forcing server returns readable JSON.

**Done (unreleased)** `tls_fetch.rs` caches `Arc<ClientConfig>` from
`rustls-platform-verifier` (OS trust store) and decodes `Content-Encoding:
gzip`. A unit test stands up a local rustls server with a private CA plus a
gzip body and fetches it through a client that trusts only that CA.

#### SUB-4 (Medium) Fetch worker panics on a poisoned queue

**Where** `crates/ice-subscription/src/lib.rs:1743,1751` (`.expect(…)` on
the queue mutex and receiver).

**Impact** One panic in any job kills the auto-update thread for the rest of
the process lifetime, silently.

**Fix** `lock().unwrap_or_else(PoisonError::into_inner)`; on `recv` error
log and exit the loop while flipping a `worker_alive: AtomicBool` that
`subscription_watch.rs` checks and uses to respawn the worker.

**Verify** Test that panics inside one job and asserts the next job still runs.

**Done (unreleased)** Fetch workers recover poisoned queue locks, catch
per-job panics, and flip `worker_alive` when the result channel closes.
`subscription_watch.rs` logs a dead pool and the outer watchdog loop
respawns after `catch_unwind`, publishing `AppState.subscription_watchdog_alive`.

#### SUB-5 (Low) Misleading error mapping

**Where** `crates/ice-subscription/src/error.rs:38` maps `Io` and
`NoActiveSubscription` to `SubFetchFailed`.

**Fix** Add `ErrorCode::SubNotFound` (`sub.not_found`) and `SubIo`
(`sub.io`); frontend copy per code.

#### SUB-6 (Low) Process-global profile cache with full clones

**Where** `crates/ice-subscription/src/merge.rs:35,85,102` — static
`PROFILE_LOAD_CACHE`, `(*profile).clone()` on every hit; `list_nodes` clones
again every 2 s.

**Fix** Return `Arc<LoadedProfile>`; own the cache in `AppState` (pass
`&ProfileCache`) instead of a static so tests and multiple instances do not
share state.

**Done (unreleased)** `ProfileCache` is owned by `AppState` (no process
static) and shared with `CaptureController`. `generate_config_with_cache`
and the start/apply wrappers take that cache. Loads return
`Arc<NormalizedProfile>`; `BuildInput.profile` is that `Arc`.

### 4.6 System proxy (`ice-proxy-sys`)

#### PROXY-1 (Medium) Corrupt backup file read as "not applied"

**Where** `crates/ice-proxy-sys/src/backup_file.rs:38-44`
(`unwrap_or(false)`) vs `recover_if_applied` which returns an error on the
same input. Callers: `orchestrate.rs:465`, `commands.rs:441,728`,
`capture.rs:1390`.

**Impact** A truncated `proxy-backup.json` (crash mid-write) makes the app
believe the system proxy is untouched → it is never restored.

**Fix** Return `Result<bool, ProxyError>` (or an enum
`Applied | NotApplied | Unknown`). For `Unknown`, probe live state
(`is_proxy_live_applied`); if it points at our port, treat as applied and
restore to defaults with a `proxy.backup_corrupt` warning; never silently
`false`.

**Verify** Test with a truncated JSON file → `Unknown` → restore path taken.

#### PROXY-2 (Low) macOS live probe is a subprocess storm

**Where** `crates/ice-proxy-sys/src/backup_file.rs:48-62` —
`is_proxy_live_applied` calls `proxy.backup()`, which on macOS is a full
`networksetup -listallnetworkservices` + 4 `get*` calls per service. The
shell's 2 s TTL cache hides it from steady-state polling, but every miss is a
burst. Note it also short-circuits on `is_proxy_applied_on_disk` (line 53),
so the PROXY-1 fix must let the `Unknown` state reach the live probe.

**Fix** Probe only the primary service (the one holding the default route)
with `-getwebproxy`/`-getsecurewebproxy`, or use the `system-configuration`
crate (SCDynamicStore) to read proxies without spawning processes.

**Done (unreleased)** Live probe maps `route -n get default` → hardware port
→ network service, then reads only that service's web/secure/SOCKS proxy.
Falls back to the first enabled service when the route cannot be resolved.

### 4.7 Architecture and module boundaries

#### ARCH-1 (Medium) `ice-config` is a shared kernel; `ice-engine` is unused

**Where** `crates/ice-config/src/lib.rs` (2337 lines) holds config
generation, `AppSettings`, `AppPaths`, pid-file handling, `tracing` init,
`AppError`/`ErrorCode` (including `core.*`/`proxy.*` codes), SSRF checks and
`cfg(target_os)` branches (`minimal_dns_block`, `tun_gate`);
`crates/ice-subscription/src/merge.rs` also branches on
`cfg!(target_os = "windows")`. `crates/ice-engine` is a workspace member but
`apps/desktop/src-tauri/Cargo.toml` does not depend on it and no production
code references it, although `architecture.md` §22 calls it the single entry
point and `ENGINE_COMPAT_CORE_VERSION` lives in `ice-config`.

**Fix**
1. Extract `crates/ice-types`: `AppError`, `ErrorCode`, `AppPaths`,
   `AppSettings` and other DTOs; no I/O.
2. Make `ice-config` a pure builder: platform choices arrive as a
   `Platform { MacOs, Windows, Linux, Android, Ios }` field of `BuildInputs`,
   no `cfg(target_os)` inside. Move pid/logging helpers to the shell
   (`src-tauri/src/runtime.rs`).
3. Decide on `ice-engine`: recommended — make `src-tauri` depend on it and
   route all config/subscription calls through the façade (that is the
   mobile-reuse story), move `ENGINE_COMPAT_CORE_VERSION` there. Otherwise
   delete the crate and §22.

**Verify** `cargo tree -p ice-config` shows no platform crates;
`rg "cfg\(target_os" crates/ice-config crates/ice-subscription` is empty.

**Done (unreleased)** `crates/ice-types` holds `ErrorCode`, `AppError`,
`AppSettings`, `AppPaths`, `HostPlatform`, listen/SSRF helpers, `UiMessage`,
and `ENGINE_COMPAT_CORE_VERSION` (no `cfg(target_os)`, no I/O). `ice-config`
is a builder: platform choices arrive as `HostPlatform`; load/save of
`settings.json` stays here. Pid-file and size-based log rotation live in
`ice-core` (the process manager needs them and must not depend on
`ice-engine`). Tracing init lives in `apps/desktop/src-tauri/src/runtime.rs`.
`src-tauri` depends on `ice-engine` for config generation and subscription
types (no direct `ice-subscription` dependency).
`ice-engine::host_platform()` is the compile-time mapping.

#### ARCH-2 (Low) Library depends on a binary crate

**Where** `crates/ice-tun-sys/Cargo.toml` → `ice-tun-launcher` for the
host-free pin helpers.

**Fix** Extract `crates/ice-tun-pin` (sha256, XML render/parse,
`command_matches_launcher`); both `ice-tun-sys` and `ice-tun-launcher`
depend on it.

**Done (unreleased)** `crates/ice-tun-pin` holds the host-free helpers;
`ice-tun-sys` and `ice-tun-launcher` both depend on it.

#### ARCH-3 (Medium) Three error-code systems

**Where** `ErrorCode` enum (`crates/ice-config/src/error.rs`, 13 variants,
no `tun.*`/`update.*`); `TunErrorCode` (`crates/ice-tun-sys/src/error.rs`);
~38 string literals like `"tun.recovery_required"` in
`apps/desktop/src-tauri/src/capture.rs` and 1 in `commands.rs`;
`AppError::with_code(code: impl Into<String>, …)` accepts anything.

**Fix** Add `Tun*` and `Update*` variants to `ErrorCode`; implement
`From<TunErrorCode> for ErrorCode`; change `with_code` to take `ErrorCode`
so the compiler flags every literal; generate `apps/desktop/src/api/errorCodes.ts`
from the enum (build script or `ts-rs`) and type `AppError.code` in the
frontend against it. Closes ORCH-2 and SUB-5.

**Verify** `rg '"[a-z]+\.[a-z_]+"' apps/desktop/src-tauri/src --type rust`
returns no error-code literals.

**Done (unreleased)** `ErrorCode` includes `tun.*` / `update.*` / helper
codes; `AppError::with_code` takes `ErrorCode`. Frontend
`apps/desktop/src/api/errorCodes.ts` is checked against `ErrorCode::ALL`.
`TunError` in `ice-types` carries `ErrorCode` (`tun.*` variants);
`ice-tun-sys` re-exports that type. There is no second `TunErrorCode` enum.

#### ARCH-4 (Low) File sizes and inlined tests

**Where** `commands.rs` 3315 lines, `capture.rs` 2844, `ice-config/lib.rs`
2337, `ice-subscription/lib.rs` 1917 (tests from line ~20 onward),
`ice-tun-sys/windows.rs` 1739; `enable_tun_inner`/`enable_tun`/
`transition_tun_settings`/`disable_tun` are each 130–160 lines.

**Fix** Split `commands/` by domain (`status`, `core`, `proxy`, `tun`,
`subscription`, `settings`, `update`, `logs`); split `capture/` into
`state`, `transition`, `recovery_bridge`, `journal_bridge`; move inline test
modules into sibling `*_tests.rs` files or `tests/`. Consider splitting
`ice-tun-sys` into `ice-tun-journal`, `ice-tun-sys` (backends) and
`ice-tun-helper-proto`.

**Done (unreleased)** Desktop IPC is `commands/{common,status,core,tun,logs,
settings,subscription,nodes}.rs` plus sibling `tests.rs`. Capture is
`capture/{mod,journal,transition,recovery}.rs` plus sibling `tests.rs`.
Config generation lives in `ice-config/src/build.rs` with
`build_tests.rs`; subscription manager + G5 tests are
`ice-subscription/src/{manager,tests_g5}.rs`; Windows TUN parsing tests are
`ice-tun-sys/src/windows_tests.rs`. TUN crates are split:
`ice-tun-journal` (journal I/O), `ice-tun-helper-proto` (IPC protocol +
install paths), `ice-tun-sys` (platform backends + coordinator).
`ice-helper` depends on the proto crate, not `ice-tun-sys`.

#### ARCH-5 (Low) Four `start_with_config` implementations

**Where** `crates/ice-tun-sys/src/coordinator.rs:50-54, 203-207, 553-557, 1092-1096`.

**Fix** One `CoreCoordinator` trait with a shared `verify_then_start` path;
fakes implement only the spawn seam.

**Done (unreleased)** Child-based runners (dev sudo / Windows elevated)
share `verify_then_start_child`; the scheduled-task path shares
`wait_for_pid_liveness`. `DeferredCoreCoordinator` stays fail-closed
without a spawn. The helper IPC runner in `ice-helper` is a different
protocol and is unchanged.

### 4.8 Performance

#### PERF-1 (Low) Per-Apply clones and file stats

**Where** `generate_config` deep-clones every outbound `Value`;
`expand_geoip_rules` calls `is_file()` per rule × code; `rule_fingerprint`
serialises each rule with `serde_json::to_string`.

**Fix** Cache the set of available GeoIP codes keyed by the directory mtime;
hash rules by streaming `serde_json::to_writer` into a `Sha256`/xxhash
hasher (or fingerprint the profile revision); pass outbounds as `Arc<Value>`.

**Done (unreleased)** GeoIP codes are cached by directory mtime;
`rule_fingerprint` streams canonical JSON into SHA-256 (`sha256:…`) and
still matches legacy canonical-JSON entries in `rules.json`.
`NormalizedOutbound.outbound` is `Arc<Value>` so profile clones are cheap.
`RuntimeConfig` keeps outbounds as `Vec<Arc<Value>>` and serializes each
`Arc` by reference, so writing `config.json` does not deep-copy outbound
trees (tag already set → cheap `Arc` clone; otherwise clone-on-write).

#### PERF-2 (Medium) Uncoordinated frontend polling

**Where** `Home.tsx` every 2 s (`get_status` + `list_nodes`); `App.tsx`
(`203-226`) polls `get_status` on non-Home pages; `Nodes.tsx` every 5 s
(`get_status` + `list_nodes` + `get_settings`); `TrafficChart` every 1 s.
Only `Logs.tsx:23` checks `document.visibilityState`. The window hides to the
tray on close (`lib.rs on_window_event`), so polling continues indefinitely
while invisible.

**Fix** One store (`useAppStore`, zustand or a context) with a single poller
that (a) pauses when `visibilityState === "hidden"` and when the backend
emits `window://hidden`, (b) subscribes to `core://status-changed` and
`traffic://sample` Tauri events and only polls as a 10 s fallback, (c) applies
the generation guard centrally (FE-1).

**Verify** DevTools: zero IPC calls while the window is hidden; ≤1 status
call per 10 s when idle and visible.

**Done (unreleased)** `RuntimeStoreProvider` is the single 10 s fallback
poller; it pauses on `visibilitychange` and `window://hidden`, and refreshes
on `core://status-changed`. Home/Nodes skip their own `getStatus` interval
when the store is present. Traffic still uses a ~1 s delta fetch (PERF-3)
while visible.

#### PERF-3 (Low) Traffic chart refetches the full window

**Fix** `get_traffic_since(cursor)` returns only new samples; chart appends.

**Done (unreleased)** `get_traffic_since` + `traffic://sample`; the chart
appends new samples and keeps a ~1 s poll as a fallback while visible.

### 4.9 Frontend

#### FE-1 (Medium) No shared server-state layer; inconsistent stale guard

`generationGuard` is used only for Home's mode switch; Home's main poll uses
its own `pollGenRef`; Settings has no guard. Fix with the store from PERF-2;
every write goes through it, every response is generation-checked.

**Done (unreleased)** `RuntimeStore` owns `bumpGeneration` / `isStale` and
the status snapshot. Home/Nodes bump on writes; Settings helper-install
polling is still local (install progress), not the idle 2 s loop.

#### FE-2 (Low) `Settings.tsx` (993 lines) reads status once

Split into `settings/{Ports,Tun,Helper,Update,Appearance}.tsx`; subscribe to
the store so `preparing`/`stopping` TUN states render
(`Settings.tsx:717-719`).

**Done (unreleased)** Settings is split into
`settings/{Appearance,Update,Tun,Helper,Ports}.tsx` and subscribes to
`RuntimeStore` so `preparing` / `stopping` update without reopening the
page. Helper install polling stays local.

#### FE-3 (Low) `useCallback(fn, [])` over changing props

**Where** `apps/desktop/src/pages/Nodes.tsx:601-607` wraps
`onSelect`/`onGroupSelect`, which are recreated by the parent every render.
Works today by accident.

**Fix** Include the deps or hold the latest prop in a ref; enable
`react-hooks/exhaustive-deps` as `error` in ESLint.

#### FE-4 (Low) Dependencies

`@tanstack/react-virtual` has zero references — remove. `tailwindcss` is in
`dependencies` — move to `devDependencies`.

#### FE-5 (Low) i18n gaps

`Home.tsx:63,65` full-width parentheses in `formatOutbound`;
`Subscriptions.tsx:137` literal `"DNS"`; core status shown as the raw enum;
English backend diagnostics rendered inside the zh UI. Fix: route all
user-visible strings through `t()`; backend returns `code` + structured
params, frontend formats.

**Done (unreleased)** Outbound labels, DNS badge, and core status go
through `t()`. Backend user-visible copy is `UiMessage { key, params }`:
parse warnings, `last_error`, `CoreState.message`, TUN unavailability, and
recovery banners. The frontend runs `t(key, params)` (`formatUiMessage`).
IPC errors still use `{ code, message }` with `formatInvokeError`.
Technical `detail` (OS / sing-box excerpts) stays in params.

#### FE-6 (Low) Tests assert on copy and classNames

Prefer `data-testid`/roles and message keys so copy/style changes do not
break ~2400 lines of page tests.

**Done (unreleased)** Page roots expose `data-testid` (`home-panel`,
`settings-panel`, …, `log-view`, `app-main`). Delay tones and the rules
pager use test ids / `data-visible` instead of Tailwind classes. Page
and helper tests look up copy through `t(key)` rather than zh literals,
including Settings form labels, helper/update copy, Rules form labels,
subscription auto-update switches, window caption buttons, delay/rule
type labels, and update progress. Layout checks use test ids
(`settings-stack`, `app-brand-row`) instead of classNames. Node tags
such as `选择组` / `自动组` remain fixture data, not UI copy.

#### FE-7 (Low) `browser-api.ts` mirrors `api/tauri.ts` by hand

Add `export const browserApi = {…} satisfies typeof import("./tauri")` so a
missing method fails type-checking. Also move the port/listen helpers out of
`lib/generationGuard.ts` (lines 21-55) into `lib/listenValidation.ts`.

**Done (unreleased)** Live Demo keeps a hand-maintained `browser-api.ts`
stand-in (Vite aliases `api/tauri` to it). `export const browserApi`
`satisfies typeof import("…/api/tauri")` so a missing value export fails
type-checking. Website `tsc` pins `react` / `react-dom` to this package's
`@types/*` and includes `vite/client` so compiling desktop UI sources does
not hit a second React identity or missing `*.png` modules. Port/listen
helpers live in `lib/listenValidation.ts`.

### 4.10 Build, CI and release

#### CI-1 (High) Integration tests never run

**Where** `scripts/gate.sh:16` and `scripts/gate-local.sh:20`
(`cargo test --workspace --lib`); `.github/workflows/ci.yml:75,78,134`.
`--lib` excludes every `crates/*/tests/*.rs`: `recovery_fault_injection.rs`
(~30 cases), `helper_e2e.rs` (4), `macos_backend.rs` / `windows_backend.rs`
(~27 non-ignored each) — the tests covering TUN recovery — are only
compiled by clippy, never executed. 19 tests are `#[ignore]`d workspace-wide.

**Fix** `cargo test --workspace` (lib + integration + doc tests) in
`gate.sh` and CI; keep `--lib` for the fast local gate but add
`cargo test -p ice-tun-sys --tests`. Add `cargo test -p ice-proxy-sys` to
the Windows job (CI-9). Document each `#[ignore]` with a reason and, where it
needs hardware, a manual checklist in `docs/testing.md`.

**Verify** CI log shows the integration binaries running with non-zero test
counts on both macOS and Windows.

#### CI-2 (Medium) No dependency auditing

Add a job running `cargo audit` (or `cargo deny check advisories licenses
bans`) and `npm audit --audit-level=high` in `apps/desktop`, plus a weekly
`schedule:` trigger.

**Done (unreleased)** `.github/workflows/audit.yml` runs `cargo deny` and
`npm audit --audit-level=high` on PR/push to `main` and weekly Mondays.

#### CI-3 (Low) Toolchain and release profile

No `rust-toolchain.toml` (floating stable); no `[profile.release]` in the
root `Cargo.toml`. Add
`rust-toolchain.toml` (`channel = "1.xx"`, `components = ["clippy", "rustfmt"]`)
and `[profile.release] lto = "thin", codegen-units = 1, strip = "symbols"`
(keep `panic = "unwind"` — the shell installs a panic hook).

#### CI-4 (Low) Action versions and concurrency

`release.yml` uses `actions/checkout@v5`, `setup-node@v5`,
`upload-artifact@v6`; `deploy-website.yml` uses `checkout@v7`,
`setup-node@v7`. Align, enable Dependabot for `github-actions`, and add
`concurrency: { group: ci-${{ github.ref }}, cancel-in-progress: true }`
to `ci.yml` (only `deploy-website.yml` has one).

#### CI-5 (High) `libcronet.dll` is not bundled

**Where** `scripts/fetch-singbox.sh` / `prepare-singbox-resource.*` copy
`libcronet.dll` into `resources/`, but
`apps/desktop/src-tauri/tauri.windows.conf.json:28-34` `bundle.resources`
does not list it; `CHANGELOG.md` (≈167-172) states it ships. sing-box 1.13's
NaiveProxy outbound needs it at runtime.

**Fix** Add `"resources/libcronet.dll"` to `bundle.resources`; add a release
smoke step that lists the NSIS payload (`7z l *.exe`) and fails if
`sing-box.exe`, `wintun.dll`, `libcronet.dll`, `ice-tun-launcher.exe` are
missing.

**Verify** Installed app directory contains `libcronet.dll`; a `naive`
outbound starts.

#### CI-6 (Low) GeoIP rule-sets committed twice

`third_party/sing-geoip/rule-set/*.srs` and
`apps/desktop/src-tauri/resources/geoip/*.srs` (30 files each) are both
tracked. Keep `third_party/` as the source, copy in `beforeBuildCommand`, and
gitignore `resources/geoip`.

**Done (unreleased)** `resources/geoip/` is gitignored; `build.rs` and
`prepare-singbox-resource.sh` still copy from `third_party/sing-geoip`.

#### CI-7 (Low) Root `LICENSE` not uploaded

`.github/workflows/release.yml:191-194` uploads `NOTICE` and
`third_party/sing-box/LICENSE` only; `docs/release-process.md:126` lists the
root `LICENSE`. Add it.

#### CI-8 (Low) `fetch-geoip.sh` has no checksum

Pin the upstream release tag and verify against a committed `CHECKSUMS`
file, as `fetch-singbox.sh` already does. Note that the sing-box trust root is
the committed checksum file — reviewing that PR *is* the trust decision;
document this in `release-process.md`.

#### CI-9 (Low) Platform coverage asymmetry

The Windows job (`ci.yml:99-145`) does not run `cargo test -p ice-proxy-sys`
(macOS does at line 78). `gate-local.sh` excludes `ice-box` and skips the
Vite build by design — document this in `release-process.md` so contributors
know desktop-crate tests only run in CI. `acceptance.rs` is fully
`#[cfg(test)]` and does not ship (confirmed; no action).

### 4.11 Documentation drift

| ID | Document says | Code does | Fix |
|---|---|---|---|
| DOC-1 | `architecture.md:880` (§21): reload is `PUT /configs?force=true` | `ice-core/src/lib.rs:364-390`: `SIGHUP` on Unix, restart on Windows; `ReloadOutcome::HotReloaded` comment (`lib.rs:109`) also says "Clash API PUT" | Done (unreleased): §9.1 / §21 and the enum comment describe SIGHUP + `GET /version` |
| DOC-2 | `architecture.md:395` (§9.1): mode switch is *not* a reload (`PATCH /configs`) | `architecture.md:551` (§12.4) and code: always rebuild + reload on 1.13.19 | Done (unreleased): §9.1 matches §12.4 |
| DOC-3 | `tun.md:90`: helper only starts/stops the core; examples use `"v": 1` (`tun.md:95-97`) | `helper_protocol.rs:33` `PROTOCOL_VERSION = 2`; `SetDns` command exists | Done (unreleased): tun.md documents v2, `SetDns`, and daemon validation |
| DOC-4 | `architecture.md:842,1155`: "reqwest / system roots" | `ureq` + custom rustls fetch with webpki roots (`tls_fetch.rs`) | Done (unreleased): §18 describes rustls-platform-verifier + gzip |
| DOC-5 | `architecture.md:201`: `nodes.json` cache in the runtime dir | `ice-subscription/src/lib.rs:223`: "legacy duplicate, no longer written" | Done (unreleased): §6 / §11.2 keep `profile.json` only |
| DOC-6 | `architecture.md:494`: over-limit → truncate + warning | Clash path hard-fails (`proxies.rs:33`) | Done (unreleased): SUB-2 truncate + warning |
| DOC-7 | `architecture.md:586`: validation checks non-empty inbounds/outbounds | `validate_config` checks key existence only | Done (unreleased): CFG-1 |
| DOC-8 | `release-process.md:126`: release uploads `LICENSE` | `release.yml` does not | Done (unreleased): CI-7 uploads root `LICENSE` |
| DOC-9 | `CHANGELOG.md`: `libcronet.dll` ships on Windows | Not in `bundle.resources` | Done (unreleased): bundled + CHANGELOG correction |

Also: §22 (`ice-engine`) is used by the desktop shell (ARCH-1); `ice-types`
holds `ErrorCode` / `AppSettings` / `AppPaths` / `HostPlatform` / the engine
pin. `settings.json` load/save stays in `ice-config`.

---

## 5. Recommended order of work

Phase 0 — small, high value, no design work (target: next patch release) — **done (unreleased)**

- CI-1 run integration tests; CI-9 Windows `ice-proxy-sys`.
- CI-5 bundle `libcronet.dll`.
- SEC-4 validate `SetDns` in the daemon.
- TUN-1 fail-closed DNS probe.
- CORE-1 idempotent stop; ORCH-2 lock-poisoned code.
- TUN-3 prefix validation.

Phase 1 — privileged boundary (target: next minor release; release notes must call it out) — **done (unreleased)**

- SEC-1 config guard in helper + launcher + subscription allowlist.
- SEC-2 protected binaries, in-memory XML, `Command` verification.
- SEC-3 config path check; SEC-5 token mode; SEC-6 adopt identity; SEC-8 test hook gating.

Phase 2 — correctness and robustness — **done (unreleased)**

- CORE-2 HTTP health; CORE-3/CORE-4 liveness semantics; CORE-5 traffic
  supervisor; CORE-7 log rotation; CORE-8 error type.
- SUB-1 atomic profile commit; SUB-2 caps; SUB-4 worker resilience; SUB-5 codes.
- CFG-1 real validation; CFG-2 settings fallback; CFG-3 atomic restore.
- PROXY-1 tri-state backup; TUN-2 journal errors.

Phase 3 — responsiveness — **done (unreleased)**

- ORCH-1 snapshot lock + status events; ORCH-3 settings patch.
- PERF-2/FE-1 shared store with visibility-aware polling; PERF-3 delta traffic.
- ARCH-3 single error-code enum + generated TS codes.
- SEC-7 helper token compare (SHA-256 + `subtle::ConstantTimeEq`).

Phase 4 — structure and hygiene — **done (unreleased)**

- ARCH-2 `ice-tun-pin`; ARCH-5 shared start liveness; SUB-3 TLS + DOC-4;
  SUB-6 Arc cache; PROXY-2 primary-service live probe; CORE-6 Windows orphan
  reclaim; PERF-1 GeoIP cache / SHA-256 fingerprints / `RuntimeConfig`
  outbound `Arc` serialize-by-ref; FE-2/3/4/5/6/7; CI-2/3/4/6/7/8; DOC-5/8.
- ARCH-1 `ice-types` (`AppSettings`) / `HostPlatform` / `ice-engine` in the
  shell; pid/logging out of `ice-config`; ARCH-4 `commands/` + `capture/`
  splits plus `ice-tun-journal` / `ice-tun-helper-proto`.

## 6. Definition of done for this review

- Every High item is either `Done` or has a linked issue with an owner before
  the next formal release (`docs/release-process.md`).
- CI executes integration tests on macOS and Windows.
- `docs/architecture.md` and `docs/tun.md` no longer contradict the code on
  the points in §4.11.
- This file's summary table reflects the current status of each ID.
