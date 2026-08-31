# TUN Mode Support Plan

Status: **proposal — shared/macOS T0 complete, T1–T4 shared complete, T5 macOS helper + packaging landed, macOS live orchestration gates G9.12 and G9.13 green; macOS is a permanently unsigned release with in-app elevated helper installation (system authorization dialog), the clean-machine gate is explicitly waived for this release, and Windows T0 pending** (the host-free Windows TUN backend — `WindowsTunBackend`, `netsh`/`route print` probes with host-free parsing tests, dev elevated runner — has landed behind the `ICE_BOX_TUN_WINDOWS_DEV` opt-in so the host spike can run as a repeatable gate via `scripts/run-acceptance-windows-tun.sh`; see `docs/design-notes/tun-windows-t0.md`. Production Windows stays fail-closed until `windows_tun_ready`).

T0 (feasibility + architecture lock) is done and recorded in
`docs/architecture.md` §24 and `docs/design-notes/tun-t0-spike.md`: schema
pin locked against the exact bundled 1.13.19 (`address` field, route-action
sniff), macOS permission model locked (privileged helper daemon runs
the core; the app is permanently unsigned — the helper is installed through
the system authorization dialog; utun/routes require root), DNS locked (no OS DNS on macOS;
router-level interception + LAN bypass), **dual-stack tun locked** (an
IPv4-only tun silently leaks IPv6 — `ipv6_address` becomes required),
ownership model locked (sing-box owns all TUN resources on macOS), control
path locked (process_name direct rules + sniff-before-domain-rules), and the
host-free journaled recovery core + fault-injection tests landed in
`crates/ice-tun-sys` (30 tests green). Windows T0 host spike remains open; it blocks only
Windows-specific TUN implementation and release gates, not shared T1 work or macOS-gated slices.

T1 (shared) landed: `TunSettings` (backward-compatible serde defaults, disabled by default,
locked CIDR / MTU / stack / interface-name validation), `CaptureIntent::{Diagnostic, Tun}`
threaded through `BuildInput`, both config builders, `generate_config`, the `ice-engine`
facade and every desktop caller, the locked TUN inbound shape + reserved bypass rules
(process_name / ip_is_private / ip_cidr → sniff → clash_mode → custom/subscription),
structural intent/config mismatch validation (`validate_config_for_intent`), and the
compile-time capability preflight (`tun_gate`: macOS green, Windows pending, other
platforms out of scope).

T2 (shared) landed: the macOS backend (`crates/ice-tun-sys`, `MacosTunBackend`) with the
native sing-box ownership contract — read-only host probes (`ifconfig -l` /
`ifconfig <name>` / `route -n get`, with host-free parsing tests), dual-stack and
`utun<N>` validation, utun collision fallback probe (higher index, fail closed),
journaled apply that records observed ownership after the elevated core starts and rolls
back (stops the core) on journal-write failure, verify (identity = name + utun index),
idempotent restore with bounded teardown wait and fail-closed `recovery_required` when
the adapter survives the core stop, and kill-9 recovery (kernel already removed the
adapter → `Cleaned`). The `CoreCoordinator` boundary (`start_with_config` / `stop`)
keeps `ice-tun-sys` free of `ice-core`; the real privileged runner (helper IPC or the
dev `sudo` wrapper) is wired by orchestration in T3 — until then transitions fail
cleanly with `tun.permission_required`. Windows/Linux hosts get the fail-closed
`UnsupportedTunBackend` (`tun.not_supported` + stable reason) via `create_backend()`.
Shared exit gate: 16 host-free macOS-backend tests (prepare/apply/rollback/verify/
restore/recover/unsupported) run deterministically on all CI platforms. The per-platform
live gate still waits for the T3 controller + runner wiring.

T3 (shared) landed: `CaptureController` in `AppState` (`capture.rs`) — the single owner of
the active backend (`TrafficCapture::{Inactive, SystemProxy, Tun}`) and the capture state
machine; the typed status payload (`traffic_capture`, `configured_tun`, `tun_status`,
`tun_interface`, `tun_error`, `capture_transition_id`, `tun_available`,
`tun_unavailable_reason`) merged into the Home status response; Home start dispatches by
`tun.enabled` through the controller, `stop_system_proxy` is retained as an IPC name but
delegates to `disable_active_backend`; the serialized settings transaction
(`settings-pending.json`: written before the transition, committed only after the
requested backend is healthy, rollback to the old backend on failure, uncertain rollback
→ `RecoveryRequired`); TUN topology changes go through the explicit
stop/reconfigure/start sequence while policy-only changes keep the normal reload path
with the controller-chosen intent; startup recovery (pending-record discard + journal
`RecoveryDriver`, never enables capture, rewrites the Diagnostic config after a clean);
the unexpected-exit watchdog path (release + verify + Diagnostic config rewrite) is wired
into `core_watch`; quit disables TUN before killing the core (bounded timeouts, warning
on unconfirmed cleanup). `ice-core` gained `PidProcess` + `CoreController::adopt_external`
so the elevated core started by the backend's coordinator is adopted by the shell
lifecycle (reload/stop/reap all work). Shared exit gate: 15 host-free controller tests
(enable/disable/switch/rollback/recovery/status/exclusivity) plus the existing suites are
green on all CI platforms. **The macOS live orchestration gate is green via the dev
`sudo` runner:** `SudoCoreCoordinator` (`crates/ice-tun-sys`) runs the bundled core as
root through `sudo -n` (never prompts; `tun.permission_required` before any OS mutation
when no cached credential / NOPASSWD rule exists), `stop()` terminates as root (a
non-root shell cannot signal a root-owned process) with bounded TERM→KILL grace, and the
backend waits bounded for the adapter to appear after the elevated start. It is an
explicit opt-in only: `create_backend` wires it when `ICE_BOX_TUN_DEV_SUDO` is set
(anything else keeps the fail-closed `DeferredCoreCoordinator`), and the destructive
live suite runs via `scripts/run-acceptance-macos-tun.sh` (G9.12: enable → mixed curl →
disable → adapter-removed verification). The permanently unsigned release uses the helper path
installed through the system authorization dialog (in-app, `install_helper` IPC, or
`scripts/install-helper-macos.sh`); no OS mutation ever happens without an explicit opt-in.

T4 (shared) landed: the Settings TUN enable switch and Home capture-status UI. Typed
frontend wrappers (`apps/desktop/src/api/tauri.ts`) expose the full §4.3 status payload
(`traffic_capture`, `configured_tun`, `tun_status`, `tun_interface`, `tun_error`,
`capture_transition_id`, `tun_available`, `tun_unavailable_reason`) and
`AppSettings.tun`; `formatInvokeError` maps the stable TUN error codes
(`tun.not_supported`, `tun.permission_required`, `tun.apply_failed`,
`tun.restore_failed`, `tun.healthcheck_failed`, `tun.recovery_required`) to actionable
Chinese text. Settings renders one TUN enable switch, clearly separate from
Rule/Global/Direct, driven by `tun_available` (unavailable → disabled switch + reason;
transition in flight → disabled + status hint; active → interface + switch back hint).
Home keeps one generic proxy-service button whose behavior follows `tun.enabled`
(controller-chosen backend); it reports the active backend and interface
(`TUN 已接管（utunN）` vs system-proxy labels), disables the control during
`preparing`/`stopping`/`recovery_required`, shows `permission_required` with a
「停用 TUN，改用系统代理」fallback action (offered only with no TUN resource active),
and shows `recovery_required` with a「重试恢复」action backed by the new `recover_tun`
IPC command (runs the journal recovery driver under the orchestration lock; never
enables capture). A configured-but-unavailable platform shows the reason and keeps the
button disabled rather than presenting a misleading enabled state. Shared exit gate:
frontend typecheck green and 155 Vitest cases cover the status/error contract and the
platform-independent state transitions.

T5 (macOS helper + packaging) landed: the production privileged helper daemon
(`crates/ice-helper`, design note `docs/design-notes/ice-helper-design.md`) replaces the
dev `sudo` runner as the elevated core context. It is a small launchd daemon whose only
capability is starting the bundled sing-box with an allowlisted config path and stopping
it (TERM→KILL with bounded grace, reaped). The wire protocol (`ice-tun-sys`,
`helper_protocol`) is one JSON frame per connection with a 16 KiB cap; security is peer
uid (`getpeereid`), a per-installation token (constant-time compare), protocol versioning,
and a canonicalized config path inside the installed data dir. The app side
(`HelperCoreCoordinator`, `helper.rs`) implements `CoreCoordinator` with bounded timeouts;
`create_backend` on macOS picks the dev `sudo` opt-in first (live gate), then the helper
when a read-only `status` probe authorizes, else the fail-closed deferred runner.
Packaging: `scripts/install-helper-macos.sh` / `uninstall-helper-macos.sh` (launchctl
bootstrap, root-owned socket + token), entitlements files, helper embedded in the bundle
resources with a CI artifact check, and a G9.13 live acceptance mode
(`scripts/run-acceptance-macos-tun.sh --helper`: install → enable → mixed curl → disable →
adapter removed → uninstall). Host-free exit gate: protocol/validation tests, client
tests, and three end-to-end helper tests (start/stop, unauthorized token, outside-path
rejection) run on all CI platforms. **In-app elevated install (unsigned elevation):** the
app is permanently unsigned, so helper installation cannot use SMAppService; instead the
`install_helper` / `uninstall_helper` IPC commands prompt the system authorization dialog
(`AuthorizationServices`, deprecated-but-functional, in `crates/ice-elevate`) and execute
the helper's own privileged `install` / `uninstall` modes as root — the single
implementation of the install logic, shared with the shell scripts. G9.13 has passed on an
authenticated macOS host using this helper. The clean-machine install/uninstall gate
is explicitly waived for this release and is not a release blocker. The existing system-proxy
release path stays green independently. Windows T5 stays blocked on `windows_tun_ready`.

Decision record: the separation between traffic capture and routing policy described in
§2 is approved. The user-facing TUN choice is a configuration switch, not a second service
button: when TUN is enabled in Settings, the existing Home "proxy service" action starts TUN;
when it is disabled, the same action starts the system proxy. `proxy_mode` remains independent.
Remaining platform and DNS details are gated independently per target platform by the
corresponding feasibility and architecture-lock slices.

Implementation locks in this plan:

- `tun.enabled` is the desired backend for the next Home service start. It is not proof that
  TUN is currently active; the status payload carries the active backend separately.
- The active backend is owned by one runtime capture controller in `AppState`. Every start,
  stop, apply, reload, quit, and crash-recovery path reads that controller; no path infers the
  active backend from `tun.enabled`, `settings.json`, or `proxy-backup.json`.
- A TUN inbound is present in `config.json` only while TUN capture is being prepared or is
  active. The diagnostic core configuration is Mixed-only. This prevents the app's automatic
  core start from silently creating an adapter or installing routes.
- Enabling or disabling TUN is a capture transition. It may reload or restart sing-box, but it
  is not complete until adapter, route, DNS, and core readiness checks agree.
- A core reload with an unchanged TUN endpoint may keep the OS capture resources in place, but
  a bounded traffic interruption is allowed while sing-box rebuilds. The UI must report
  `preparing` during that interval; there is no zero-downtime promise in the first release.
- TUN ownership and recovery live in a separate `ice-tun-sys` boundary. System-proxy backup
  data is never reused for TUN state.
- An unexpected sing-box exit is an immediate capture transition while the app is still running:
  the controller releases and verifies TUN resources before reporting the capture as disabled.
  Startup recovery is a second line of defense, not the only cleanup path.

This document is the implementation plan for adding TUN / transparent proxy support to
ice-box. It is intentionally separate from `docs/architecture.md`: the current architecture
explicitly excludes TUN from v1, so the architecture document must be amended at the first
implementation slice, before code that depends on the new contract is merged.

## 1. Current baseline

The repository already has the pieces that should remain the foundation:

| Area | Current implementation | Consequence for TUN |
|------|------------------------|---------------------|
| Desktop shell | Tauri 2 + React in `apps/desktop` | UI continues to call Rust commands only. No TUN protocol or privileged operation belongs in React. |
| Forwarding core | Pinned `sing-box` subprocess (`1.13.19`) managed by `crates/ice-core` | TUN should use a sing-box `tun` inbound first; do not implement protocol forwarding in ice-box. |
| Config pipeline | `ice-config` / `ice-engine` build a final JSON config from `LocalTemplate` and a normalized profile | Add a typed TUN section to the template and keep platform-specific process/privilege work outside the config engine. |
| Existing ingress | One `mixed` inbound, optionally bound to `0.0.0.0` for LAN sharing | TUN is a second ingress/traffic-capture path, not a replacement for Mixed during the first migration slice. |
| Routing modes | `ProxyMode::{Rule, Global, Direct}` are orthogonal routing policies, persisted in `settings.json` and emitted through `clash_mode` rules | Keep these policies. Add a separate traffic-capture selection instead of overloading `ProxyMode`. |
| System proxy | `ice-proxy-sys` backs up, applies, restores macOS and Windows settings; Linux uses `NoopSystemProxy` | TUN needs a separate lifecycle and recovery record. Never use the HTTP proxy backup file to represent TUN state. |
| Orchestration | `apps/desktop/src-tauri/src/orchestrate.rs` serializes start/stop/apply and reloads the core | Extend the same transaction boundary so TUN enable/disable cannot race with config reload or quit. |
| Recovery | `proxy-backup.json`, `sing-box.pid`, startup restore, and an unexpected-exit watchdog already exist | Add a TUN mutation journal that records ownership and every completed OS mutation. |
| Health checks | TCP probe to the local Clash API, 5 second timeout | Keep the Clash API probe, and add a TUN-specific readiness probe that proves the adapter and route state are usable. |
| UI | Home page has one generic proxy-service toggle and the Rule/Global/Direct control; Settings edits ports and LAN sharing | Add the TUN enable switch in Settings, active-backend status, permission errors, and a safe fallback path without exposing platform APIs. |
| CI and packaging | Linux/macOS/Windows gates; macOS `.app`/`.dmg`; Windows NSIS; sing-box and GeoIP resources are bundled | TUN-specific driver, entitlements, in-app elevated helper installation, and elevated-install tests are not present and must be added before release. |

Important existing constraints to preserve:

- The Clash API remains loopback-only.
- Subscription fetches are direct and must not depend on the configured system proxy.
- Config files are written atomically and the old runtime config is retained as `.bak`.
- A failed apply must not leave a partially enabled proxy or an untracked TUN route.
- No Git commit or release is part of this planning task.

## 2. Approved product model

Treat **traffic capture selection** and **routing policy** as two separate concerns. The
traffic-capture selection is persisted as a TUN configuration switch; the active service
backend is derived from that switch:

```text
tun.enabled: true  -> proxy service uses tun
tun.enabled: false -> proxy service uses system_proxy
proxy_mode:  rule | global | direct
```

`proxy_mode` keeps its current meaning. `tun.enabled` decides how applications enter sing-box
when the user turns on the proxy service, but the mapping is executable only when the current
platform's TUN gate is green. If `tun.enabled=true` while that gate is pending or failed, the
setting remains a desired value, the service start returns the documented unavailable error, and
the UI may offer system-proxy fallback only when no TUN resource is active.
The initial product behavior should be:

1. The Home page keeps one generic proxy-service start/stop control.
2. With `tun.enabled = false`, starting the proxy service follows the existing macOS/Windows
   system-proxy flow.
3. With `tun.enabled = true` and the platform gate green, starting the proxy service enables
   transparent capture through the sing-box TUN inbound instead of changing the OS
   HTTP/HTTPS/SOCKS proxy.
4. Stopping the proxy service disables whichever backend is active. The core may remain
   running for diagnostics, but no OS traffic capture remains enabled.
5. While TUN is active, the runtime config contains both `mixed` and `tun`, so users can use
   Mixed for diagnostics and local tools. While the service is stopped, the core may remain
   running with a Mixed-only diagnostic config; the TUN inbound is absent, so it cannot create
   a second capture path accidentally.
6. The default is `tun.enabled = false`. Existing installations therefore retain system-proxy
   behavior, and TUN is never enabled by a settings migration implicitly.
7. `system_proxy` and `tun` remain mutually exclusive at the OS boundary. Changing
   `tun.enabled` while the proxy service is active is a serialized backend transition: the old
   capture is confirmed disabled before the new one is prepared. If the transition fails, the
   old backend is restored; if restoration is uncertain, both backends stay disabled and the UI
   shows recovery-required state.

The runtime controller enforces this exclusivity: `enable_tun` must reject while a system-proxy
record is active, and `enable_system_proxy` must reject while TUN is `Preparing`, `Enabled`, or
`Stopping`. A successful disable must clear and verify the old backend before either controller
can prepare the other one.

This model avoids conflating "send everything through the proxy" (`global`) with "how traffic
reaches the proxy" (system proxy versus TUN), while keeping the main service action simple and
predictable. A future per-app mode can still be added without changing `proxy_mode`.

## 3. Non-negotiable feasibility spike

Before enabling TUN on a platform, run a small spike against the exact bundled sing-box version
(`1.13.19`) on that platform's real host. The shared schema and host-free recovery checks are
prerequisites for common T1 work; a platform spike gates only that platform's TUN activation,
platform-specific fields, and platform release work. A failed Windows spike therefore does not
block shared config types/tests or an already-qualified macOS path.

### 3.1 sing-box config capability

Confirm the pinned binary accepts and starts with a minimal config containing. This is the shared
schema gate; any platform-only field or behavior remains subject to that platform's own gate:

- one `mixed` inbound and one `tun` inbound;
- the intended `interface_name`, address/prefix, MTU, stack, `auto_route`, and `strict_route`
  fields;
- the existing `route`/`dns` blocks and `clash_mode` rules;
- IPv4 and IPv6 behavior;
- the chosen DNS interception/hijack behavior, if any.

Record the exact accepted JSON shape and startup output in the plan's follow-up design note.
Do not infer field names from a newer sing-box release. If `1.13.19` cannot provide the
required behavior on one platform, block only that platform's TUN slices and decide whether to
upgrade the pinned core or add a platform adapter before enabling TUN there.

### 3.2 Platform capability and permission model

Run this smoke test independently for each supported platform. Each result produces a separate
platform gate (`macos_tun_ready` or `windows_tun_ready`):

- Can the bundled core create the TUN adapter without an external driver?
- Which operations require elevation: adapter creation, route changes, DNS changes, or all of
  them?
- Does stopping sing-box remove the adapter and all routes after a normal stop?
- What remains after `kill -9` / task termination?
- Can the app request elevation once and keep the main Tauri process unelevated?
- For a future broad distribution, what must be installed or authorized on a clean machine?

The result for each platform chooses independently between the following implementation options:

1. **Native sing-box path (preferred):** sing-box owns the adapter and route changes; ice-box
   only coordinates permission and verifies state.
2. **Small privileged helper:** a narrowly scoped helper performs adapter/route/DNS
   operations and exposes an authenticated local IPC contract. The UI process remains
   unelevated. On the unsigned macOS release, the helper is installed through the system
   authorization dialog on first use (in-app) or via the install script.
3. **Platform network extension/driver package:** required if native sing-box cannot meet the
   platform security model. This expands installer and release work and must be
   explicitly approved.

The plan must not assume that a WinTUN DLL, a macOS Network Extension entitlement, or a UAC
policy is available merely because the OS supports TUN. A missing or failed platform gate keeps
`tun_available=false` on that platform while other qualified platforms may proceed.

### 3.3 Self-traffic and control-path spike

The first-release bypass list must be validated as an end-to-end behavior, not only emitted as
route rules. With TUN active, exercise all of the following separately on each platform host;
passing one platform does not imply the other platform passes:

- fetch and update a subscription whose hostname resolves to changing addresses;
- call the loopback Clash API while traffic is being captured;
- perform DNS queries used by both the app and sing-box;
- complete helper IPC and any permission handshake, when a helper path is selected; and
- reload the core and change `ProxyMode` without sending the control path through the proxy.

A fixed IP/CIDR exclusion alone is insufficient for arbitrary subscription URLs. T0 must prove
the selected mechanism for control traffic (for example a process/socket bypass, a dedicated
direct resolver/path, or a helper-owned channel) and document its limits. If no mechanism can
reliably bypass app/core traffic on a platform, that platform is not eligible for T1 TUN
activation. These tests belong in the T0 exit gate and in live acceptance; they must not be
deferred until the UI slice.

## 4. Target architecture changes

### 4.1 Configuration and data types

Add typed, serializable settings in `crates/ice-config/src/settings.rs` and the shared config
types. The persisted setting has one user-facing enable switch. The remaining fields below are
validated implementation parameters with locked defaults; they are not additional capture
modes and are not exposed as free-form UI inputs in the first release (field placement remains
provisional until the spike):

```rust
struct TunSettings {
    enabled: bool,
    interface_name: Option<String>,
    ipv4_address: String,
    ipv6_address: Option<String>,
    mtu: u16,
    auto_route: bool,
    strict_route: bool,
    stack: String,
    dns_hijack: bool,
}
```

The orchestration layer may use an internal derived enum such as `CaptureBackend::{SystemProxy,
Tun}`, but this is not a third user-facing mode and must not be persisted as a competing source
of truth.

The final fields and defaults must come from the feasibility spike. Required rules:

- Existing `settings.json` files load unchanged; missing TUN fields mean disabled.
- Validate addresses, prefixes, MTU, and interface names before writing settings.
- Reject a TUN configuration that would bind the Clash API away from loopback.
- Keep user settings separate from generated sing-box JSON.
- Never store credentials, subscription URLs, or arbitrary shell commands in TUN settings.

`LocalTemplate` should carry the validated TUN parameters. Add a non-persisted
`CaptureIntent::{Diagnostic, Tun}` to the shared config input (`BuildInput`, and the
direct-only builder) and require callers to pass it explicitly. Platform-specific handles
(adapter IDs, route tokens, helper session IDs) stay in `ice-tun-sys`. Address fields must be
documented as CIDRs (for example `10.0.0.2/30`), not ambiguous host addresses. The controller,
not the config crate, owns runtime handles.

### 4.2 Generated sing-box config

Extend `crates/ice-config/src/lib.rs` so the generated config can contain a TUN inbound while
retaining the current Mixed inbound. The builder must accept an explicit runtime capture
intent (`Diagnostic` or `Tun`) in addition to the persisted settings. The intent is supplied
by orchestration and is never inferred from `tun.enabled` alone. The same intent must be
threaded through `generate_config`, `build_direct_only_config`, the `ice-engine` facade, and
all subscription/config call sites.

The generated configurations are:

- `Diagnostic`: Mixed inbound only; used by automatic core start and by a stopped proxy service.
- `Tun`: Mixed plus TUN inbounds; used only during a TUN capture transition and while TUN is
  active.

The builder must:

1. Emit stable tags such as `mixed-in` and `tun-in`.
2. Emit only fields supported by sing-box `1.13.19` and the selected platform. The shared
   builder may carry a capability profile, but a platform-specific TUN config must not be
   generated or activated until that platform's T0 gate is green.
3. Put reserved self-traffic and safety-bypass rules first, then the `clash_mode` rules, then
   custom and subscription rules. TUN and Mixed use the same Rule/Global/Direct semantics for
   non-bypass traffic. The bypass rules are deliberate safety overrides so Global/Direct cannot
   loop the control path or capture the TUN's own endpoint. This ordering supersedes the
   v1 `clash_mode`-first invariant for a `Tun` config; `Diagnostic` retains the existing
   Mixed-only behavior.
4. Add explicit structured route exclusions for the loopback Clash API, Mixed listener, TUN
   CIDR/endpoint, loopback and link-local destinations, RFC1918/RFC4193 private networks,
   multicast, DNS resolver traffic, and the sing-box/ice-box control path. The exact sing-box
   JSON fields are locked by T0; arbitrary user-supplied route targets are not accepted.
5. Define DNS behavior explicitly. A TUN implementation that captures TCP but leaks or loops
   DNS is not complete.
6. Keep `route.final`, selector tags, GeoIP rule sets, custom rule validation, and direct-only
   fallback working exactly as they do for Mixed. The direct-only fallback must also honor the
   selected `Diagnostic`/`Tun` intent and the same safety exclusions; an empty/no-node profile
   must not silently downgrade a requested `Tun` intent to Mixed-only.
7. Validate that every inbound, route exclusion, DNS reference, and outbound reference is
   internally consistent before writing `config.json`. A config built with `Diagnostic` must not
   contain a TUN inbound, and a Mixed-only config must not be handed to a TUN controller as an
   activation config.

Add config tests for Diagnostic/Tun intent, each `ProxyMode`, IPv4-only, dual-stack, invalid
MTU/address, route-rule ordering, reserved-route exclusions, and direct-only profiles. Include
a golden JSON fixture only after the exact sing-box schema is locked.

### 4.3 Runtime lifecycle and state

Extend the orchestration state machine with a TUN capture state separate from core state and
the existing system-proxy record:

```text
Capture: Disabled -> Preparing -> Enabled -> Stopping -> Disabled
                         \-> PermissionRequired / Error / RecoveryRequired
```

Core `Running` is a prerequisite, but it is not equivalent to TUN `Enabled`.
`RecoveryRequired` means that ownership or cleanup could not be verified. It is a fail-closed
state: both capture backends remain disabled and new TUN activation is rejected until an
explicit recovery attempt succeeds.

Add one runtime `CaptureController` to `AppState`. It owns `active_backend`, `tun_status`, the
current transition ID, and the handles/journal needed for recovery. Its intent-level contract
must include `active_backend()`, `enable_tun()`, `disable_active_backend()`, `verify()`, and
`recover()`. `orchestrate_apply` and every command that can start, stop, reload, switch mode,
quit, or react to a core exit must receive/use this controller and assert that the generated
config intent matches the active transition. The existing IPC command names may remain for
compatibility, but their Home service implementation must dispatch through this single
backend-independent controller; it must never call system-proxy code based only on settings.

Capture transition ordering is part of the contract:

- **Enable TUN:** acquire the orchestration lock, create the `preparing` journal, prepare and
  validate the Tun config, then reload/restart the core with the TUN inbound. Apply or coordinate
  the platform resources according to the T0 ownership decision, verify the Clash API and TUN
  health, and only then mark the journal `applied` and capture `enabled`.
- **Disable TUN:** mark capture `stopping`, create a pending Diagnostic config, and keep the
  journal in `restoring`. Ask the selected owner to release capture: the native sing-box path
  stops/restarts the core with the Diagnostic config, while a helper path restores its routes,
  DNS, and adapter before (or as part of) that core transition. Verify that no owned
  route/DNS/adapter remains active, then ensure the core is Running on the Mixed-only config.
  Mark the journal `clean` and capture `disabled` only after both checks pass. If either check
  fails, stop the core if necessary to fail closed and retain the journal for recovery.
- **Reload with an unchanged endpoint:** retain the owned resources where the platform permits,
  set capture to `preparing`, reload/restart, and verify the resources again before returning to
  `enabled`. A restart that removes resources is treated as a disable/re-apply operation, not
  as a successful transparent reload.
- **Capture-topology change:** a TUN endpoint, address, MTU, stack, DNS interception setting,
  or route-exclusion/auto-route policy change changes the capture topology. Complete the disable
  sequence, write and validate the new Tun config, then perform the enable sequence. There is no
  in-place mutation of an active endpoint or topology.
- **Policy-only change:** a subscription rule, custom rule, node/group selection, or
  `ProxyMode` change does not change the TUN topology. It may use the normal core reload/live
  selection path while TUN remains owned, but capture still moves to `preparing` until the
  Clash API and TUN health checks pass. If the core restart removes the resources, fall back to
  the disable/re-apply sequence above.

Update `apps/desktop/src-tauri/src/orchestrate.rs` and commands so that:

- automatic core start always uses the Diagnostic (Mixed-only) config and never enables a
  capture backend;
- the existing Home proxy-service `start` command ensures the core is running on Diagnostic when
  needed, then dispatches to the controller's configured backend according to `tun.enabled`.
  TUN activation first writes/validates the Tun config, then performs the controller transition
  and readiness checks;
- the existing Home proxy-service `stop_system_proxy` command is retained as an IPC compatibility
  name but delegates to `disable_active_backend`: it restores the OS proxy when system proxy is
  active, or removes the TUN inbound/config and tears down routes/DNS/adapter when TUN is active.
  The core may remain Running on the resulting Diagnostic config;
- changing `tun.enabled` while the proxy service is active is a serialized transaction: write a
  pending settings record, disable the old backend, rebuild the requested capture config, enable
  the new backend, verify it, and only then commit `settings.json`. On failure, restore the old
  config and backend; if rollback cannot be confirmed, leave both backends disabled and retain a
  recovery record instead of claiming success;
- changing ports, subscriptions, rules, or `ProxyMode` while TUN is enabled may use the normal
  core reload path when the TUN endpoint is unchanged. Capture remains logically owned by the
  controller, but status moves to `preparing` until the Clash API and TUN health checks pass;
  if the reload path restarts sing-box, the controller re-verifies/re-applies owned resources
  before returning to `enabled`;
- a TUN endpoint, address, MTU, stack, DNS interception setting, or route-exclusion/auto-route
  policy change follows the explicit capture-topology stop/reconfigure/start sequence and is not
  hidden behind the system-proxy synchronization helper;
- core reload failure rolls back `config.json`, then restores the prior capture config and
  backend. A failed rollback is a persistent `tun.restore_failed` state, not a successful
  apply warning. If the core is no longer healthy, owned capture resources are restored or the
  core is stopped before any fallback is attempted;
- quit always disables TUN before killing sing-box, with a bounded timeout and a visible warning
  if cleanup cannot be confirmed.
- if sing-box exits unexpectedly while TUN is active, the watchdog acquires the orchestration
  lock, marks capture `stopping`, and runs the controller's idempotent restore/verify sequence
  immediately. The core-exit path must not only restore `proxy-backup.json`; it must also write
  and validate the Mixed-only Diagnostic config so a later automatic core start cannot recreate
  TUN from a stale runtime file. If cleanup succeeds, report capture `disabled`/core `error`;
  if cleanup is uncertain, stop any remaining core, keep both backends disabled, persist
  `RecoveryRequired`, and retry on later watchdog ticks and the next startup.

Settings persistence is transactional. When the service is inactive, a validated candidate may
be committed directly. When capture is active, write a pending record containing the old/new
settings hashes and runtime config paths, perform the transition above, atomically commit the
candidate only after health checks pass, and clear the pending record. On any error restore the
old settings/config/backend before clearing the record. If the process dies with a pending record,
startup treats it as an interrupted transition, restores the old state without enabling TUN,
and surfaces the recovery warning.

Add a typed status payload, for example:

```text
traffic_capture: inactive | system_proxy | tun
configured_tun: boolean
tun_status: disabled | preparing | enabled | stopping | permission_required | error | recovery_required
tun_interface: optional string
tun_error: optional stable AppError payload
capture_transition_id: optional opaque identifier
tun_available: boolean
tun_unavailable_reason: optional stable message
```

`traffic_capture` is derived only from the runtime controller. When cleanup is uncertain it is
`inactive` (no backend is claimed) while `tun_status=recovery_required`; the frontend must use
the latter to block fallback and show recovery. The Home page should show the active backend as
status, while the Settings page owns the
`configured_tun` switch. `configured_tun` is the committed desired setting; it must not be
changed to `true` until an active transition succeeds when the service is already running.
Do not expose raw route tables or privileged helper internals to the frontend.

### 4.4 Crash recovery and ownership

Add a dedicated data file, for example `tun-state.json`, with an atomic write protocol. The file
is a mutation journal, not merely a final-state snapshot:

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

Before the first OS mutation, atomically write `state=preparing` and the transition ID. After
each interface/address/route/DNS mutation, atomically update the journal and
`last_completed_step`. The controller must record enough original DNS state to restore it, not
just a boolean. Route records include the platform route table/interface/metric where relevant.
The contract must state which values ice-box owns and can safely remove; an unverified resource
is never deleted. DNS restore is compare-before-restore: restore `dns_before` only when the
current platform DNS still matches the journal's `dns_after` snapshot. If another VPN, user, or
system service changed DNS while TUN was active, preserve that external change, do not overwrite
it with stale data, and enter `RecoveryRequired` until the user explicitly retries recovery.
The same ownership check applies to routes and adapter identity.

On startup:

1. Acquire the same orchestration lock used by commands and reclaim/stop any orphan sing-box
   process before touching TUN resources. The controller then reads the journal and verifies
   whether sing-box-owned resources disappeared or remain; killing the core is not treated as
   proof that helper-owned resources were released.
2. Read `tun-state.json` and handle `preparing`, `applied`, `restoring`, `error`, and
   `recovery_required` states.
3. Verify the owner token, adapter identity, and route ownership before removing anything.
4. Resume an idempotent restore from the last completed journal step; mark `clean` only after
   adapter/routes/DNS verification succeeds.
5. Resolve an interrupted settings pending record by restoring the previous committed settings;
   never auto-enable TUN during recovery, even when `settings.json` has `tun.enabled=true`.
6. Surface a persistent warning and block new TUN activation when cleanup cannot be confirmed.

System proxy recovery remains independent and continues to use `proxy-backup.json`.

### 4.5 Platform boundary

`ice-proxy-sys` remains limited to OS HTTP/HTTPS/SOCKS settings. Add a separate `ice-tun-sys`
crate for adapter/route/DNS ownership, platform permissions, and the recovery journal. This
keeps the existing proxy backup contract intact even if the T0 spike selects native sing-box
ownership on one platform and a helper on the other.

The `ice-tun-sys` platform-backend trait should be mockable and should expose only intent-level
operations. It is distinct from the runtime `CaptureController` in §4.3:

```text
capability() -> TunCapability
prepare(config) -> Result<PreparedTun, TunError>
apply(prepared) -> Result<AppliedTun, TunError>
verify(applied) -> Result<TunHealth, TunError>
restore(applied) -> Result<(), TunError>
recover() -> Result<RecoveryOutcome, TunError>
```

`TunHealth` must include at least: adapter identity and up-state, expected owned CIDRs, route
ownership/availability, DNS interception or bypass status, and a loop/no-route-to-control-path
check. A Clash API TCP success alone is never sufficient.

`prepare` must be side-effect free. `apply`, `verify`, `restore`, and `recover` are idempotent and
update the journal at each mutation boundary. `recover` is the explicit retry operation used by
the watchdog/startup path and must return whether all owned resources are confirmed clean; it must
never enable capture. In the native sing-box path, `apply`/`restore` coordinate the core and
record/verify resources owned by sing-box; in a helper path they perform the explicit OS
mutations. No platform command strings should leak into `ice-config` or the React layer. On
unsupported platforms the backend returns a stable `tun.not_supported` error and reports
`tun_available=false` with a human-readable reason.

### 4.6 Frontend and IPC

Add typed Tauri wrappers in `apps/desktop/src/api/tauri.ts` and a Home/Settings control that:

- adds one TUN enable switch in Settings, clearly separate from Rule/Global/Direct;
- keeps one generic Home proxy-service start/stop button whose behavior follows the saved TUN
  switch;
- shows `permission_required` as an actionable state, not a generic core failure;
- shows `recovery_required` as a separate actionable recovery state, not as permission or core
  failure;
- disables conflicting controls while a transition is in progress;
- shows whether TUN is enabled and which interface is active;
- offers a clear fallback action that disables the TUN setting and starts the system proxy when
  TUN setup fails;
- does not claim "all traffic" until the readiness checks pass.
- fallback to system proxy is offered only after TUN cleanup is confirmed. If cleanup is
  uncertain, the fallback action is replaced by a recovery action and no second backend is
  enabled.

Keep the existing generic proxy-service semantics. A stopped service must not be interpreted as
a stopped core, and a running core must not be interpreted as active TUN.

## 5. Implementation slices

### Slice T0: shared and per-platform feasibility gates

#### T0-S: shared contract gate

- Lock the common `sing-box` 1.13.19 schema, `CaptureIntent`, status/error contract, and
  operation ordering that do not depend on host APIs.
- Inject failures after every journaled mutation in a host-free fake controller and prove that
  recovery is idempotent.
- Lock the rule that one owner is responsible for each adapter/route/DNS resource; platform
  implementations may choose different owners, but ownership cannot be split within a platform.
- Update `docs/architecture.md` with the shared product model, state machine, and data contract.

**T0-S exit gate:** common config shape, intent mismatch validation, journal format, rollback
ordering, timeout behavior, and host-free fault-injection recovery tests pass. T0-S unlocks the
platform-neutral portion of T1.

#### T0-macOS and T0-Windows: independent platform gates

For each platform, run the exact-version config, permission, cleanup, and self-traffic spike
from §3, decide native versus helper versus extension/driver, decide DNS ownership, and record
the accepted JSON fields and host prerequisites in the platform design note. The platform gate
must prove minimal TUN startup, permission behavior, normal and forced-stop cleanup, and the
self-traffic/control-path checks (subscription fetch, DNS, Clash API, reload, and helper IPC
where applicable).

**Per-platform exit gate:** mark `macos_tun_ready` or `windows_tun_ready` only after that
platform's spike and live cleanup checks pass. A failed or pending platform gate sets
`tun_available=false` on that platform and blocks only that platform's TUN activation and
platform-specific implementation/release slices.

### Slice T1: typed settings and config generation

- Add `TunSettings` with an `enabled` switch and backward-compatible serde defaults.
- Add the `CaptureIntent::{Diagnostic, Tun}` input to the config builder, direct-only fallback,
  engine facade, and every caller; structural validation must reject an intent/config mismatch.
- Add route exclusion and DNS rules required by the spike.
- Add a capability preflight so unsupported or not-yet-qualified hosts never attempt a TUN
  transition. T1 shared code may land after T0-S; platform-specific TUN fields are enabled only
  behind the corresponding `macos_tun_ready`/`windows_tun_ready` gate.
- Add unit/golden tests and update engine facade tests.

**Shared exit gate:** `cargo test` proves all existing Mixed behavior is unchanged and every
platform-neutral generated config passes local structural validation.

**Per-platform exit gate:** a platform may generate and activate its TUN config only after its
platform gate is green and its platform-specific schema/field tests pass.

### Slice T2: platform controller and permission flow

- Implement the selected macOS backend only after `macos_tun_ready`; its gate is independent of
  Windows.
- Implement the selected Windows backend only after `windows_tun_ready`, including
  driver/adapter discovery and UAC/helper interaction if required; a pending Windows gate does
  not block macOS work.
- Add mock implementations for orchestration tests.
- Implement journal updates before and after each OS mutation, with ownership checks and
  idempotent restore.
- Add stable error codes (`tun.not_supported`, `tun.permission_required`, `tun.apply_failed`,
  `tun.restore_failed`, `tun.healthcheck_failed`, `tun.recovery_required`).

**Shared exit gate:** mocked enable/disable, rollback, and error mapping are deterministic on
all CI platforms.

**Per-platform exit gate:** each platform's live controller tests pass on that platform's host;
the other platform remains independently gated.

### Slice T3: orchestration, reload, and recovery

- Add capture state to `AppState` and the status response.
- Serialize capture transitions with the existing orchestration lock.
- Implement shared enable/disable/switch commands and config reload coordination. Platform
  activation is capability-gated: a command on a platform whose gate is pending or failed must
  return `tun.not_supported`/the documented unavailable reason without mutating the host.
- Add `tun-state.json` recovery and forced-exit cleanup, including interrupted `preparing` and
  `restoring` operations.
- Add the settings transaction journal/pending record and commit `settings.json` only after the
  requested backend is healthy.
- Verify subscription update, rule update, node selection, and `ProxyMode` switches while TUN
  is enabled.

**Shared exit gate:** every mocked failed transition leaves either the old capture mode working
or both modes disabled; no test leaves a route or adapter behind; a failed settings transaction
cannot commit the new desired backend.

**Per-platform exit gate:** live orchestration, forced-exit cleanup, and recovery pass only on
platforms whose T0/T2 gates are green. A pending platform gate keeps that platform fail-closed
without blocking the other platform's orchestration work.

### Slice T4: UI and user-facing documentation ✅ landed

- Add the TUN enable switch in Settings, plus active-backend status details on Home. ✅
- Keep the Home page to one generic proxy-service start/stop control; do not add a separate TUN
  service button. ✅
- Add permission and fallback UX. ✅
- Update README, architecture docs, troubleshooting, and platform prerequisites. ✅
- Keep all user-visible technical messages precise about whether core, capture, and routing are
  active.
- Render the TUN switch and activation controls from `tun_available`; a platform with a pending
  or failed gate may show the capability as unavailable, but must not present a misleading
  enabled state. Shared UI/API types and tests may land before every platform gate is green.

**Shared exit gate:** frontend typecheck and Vitest cover the status/error contract and all
platform-independent state transitions.

**Per-platform exit gate:** manual desktop checks cover TUN transitions only on platforms with a
green platform gate.

### Slice T5: packaging and release readiness ✅ (macOS code and packaging landed; in-app elevated helper installation landed; clean-machine gate waived for this release)

- macOS: after `macos_tun_ready`, add required entitlements and helper/extension embedding. ✅
  (entitlements, bundle embedding + CI check, in-app elevated install/uninstall via the
  system authorization dialog, and helper daemon; the app is permanently unsigned — the
  in-app installer replaces SMAppService, which requires signing)
- Windows: after `windows_tun_ready`, bundle/install the required driver or helper, define
  NSIS elevation behavior, and verify uninstall cleanup. ⏳ (blocked on `windows_tun_ready`)
- Update resource preparation scripts and CI artifact checks. ✅

**macOS exit gate for this release:** G9.12, G9.13, and unsigned packaging.
The clean-machine install/uninstall gate is intentionally excluded
and is not a prerequisite for the macOS TUN release. The existing system-proxy
release path remains green independently; Windows remains blocked on `windows_tun_ready`.

## 6. Test and acceptance matrix

### Automated, no privileged host required

- Settings migration: old `settings.json` loads with TUN disabled.
- Validation: invalid addresses, prefixes, MTU, interface names, and conflicting ports fail
  before any disk or OS mutation.
- Config generation: Diagnostic has no TUN inbound; Tun has both inbounds, reserved bypass rules
  precede `clash_mode`, DNS references resolve, selector/direct-only fallback remains valid, and
  every builder/direct-only/engine call site preserves the explicit capture intent.
- Platform capability and error-code mapping with mocks.
- Orchestration transaction tests: enable, disable, switch, rollback, reload with bounded
  interruption, quit, and concurrent command serialization.
- Recovery journal atomicity, crash-after-each-mutation, stale/foreign adapter protection,
  unexpected core exit cleanup while the app remains open, and idempotent cleanup.
- Settings pending/commit/rollback behavior, including a process crash before commit.
- DNS compare-before-restore: external DNS changes are preserved and produce
  `recovery_required` rather than being overwritten.
- Frontend API/status/error states and busy-state behavior.

### Live macOS acceptance (ignored/manual, real host)

This suite is gated only by `macos_tun_ready`; its result does not wait for the Windows gate
and does not qualify Windows. It runs via `scripts/run-acceptance-macos-tun.sh`, which
opts into the dev `sudo` runner (`ICE_BOX_TUN_DEV_SUDO`) and preflights `sudo -n`
(cached credential or NOPASSWD); G9.12 covers the enable → traffic → disable roundtrip,
and the remaining manual items below are run on a real host by the release gate.

- TUN adapter creation and expected interface/address.
- IPv4 TCP and UDP traffic through Rule, Global, and Direct modes.
- DNS behavior, including no DNS loop. IPv6 is tested when the T0 capability probe marks it
  supported; otherwise the UI must show the explicit IPv6 limitation.
- Local/private/link-local destinations follow the documented bypass policy.
- Subscription fetch/update still works while TUN is active.
- Core reload, mode change, node/group selection, and port change while TUN is active.
- Normal stop restores routes/DNS/interface; forced termination is cleaned on next launch.
- Permission denial leaves the system unchanged and offers system-proxy fallback.
- While TUN is active, subscription fetch/update, DNS, Clash API calls, core reload, and any
  helper IPC use the proven direct control path and do not loop through the proxy.
- If DNS or route state was changed externally during the run, stop/restore preserves the
  external state and reports recovery-required instead of overwriting it.

### Live Windows acceptance (ignored/manual, real host)

This suite is gated only by `windows_tun_ready`; while that gate is pending or failed, Windows
must remain `tun_available=false` and the suite is not a prerequisite for macOS TUN work.

- Wintun/adapter discovery or helper installation on a clean machine.
- UAC prompt and least-privilege behavior.
- IPv4, UDP, DNS, local bypass, and no-loop checks. IPv6 is tested when supported by the locked
  host path, otherwise it is reported as unavailable rather than silently claimed.
- Same reload, mode, subscription, stop, forced-termination, and fallback cases as macOS.
- NSIS install/upgrade/uninstall leaves no stale driver, helper, adapter, or route.
- While TUN is active, subscription fetch/update, DNS, Clash API calls, core reload, and any
  helper IPC use the proven direct control path and do not loop through the proxy.
- If DNS or route state was changed externally during the run, stop/restore preserves the
  external state and reports recovery-required instead of overwriting it.

Add dedicated scripts rather than hiding destructive host tests inside the normal gate, for
example `scripts/run-acceptance-macos-tun.sh` and `scripts/run-acceptance-windows-tun.sh`.
The ordinary `scripts/gate-local.sh` / `scripts/gate.sh` must remain non-privileged and must not
mutate host routes or proxy settings.

## 7. Security and failure requirements

- TUN activation is an explicit user action; no silent enablement on upgrade or startup.
- Any privileged component is versioned with the app and exposes a narrow authenticated
  IPC surface. It must reject arbitrary paths, commands, route targets, and interface names.
- Only loopback control endpoints are accepted. `allow_lan` must not expose the Clash API or a
  privileged control socket.
- Route/DNS cleanup must verify ownership before deletion, so ice-box cannot remove a user's
  unrelated VPN state.
- Subscription fetch and core control traffic need an explicit self-traffic bypass policy to
  prevent TUN loops and update deadlocks.
- On any uncertain cleanup state, fail closed for new TUN activation and show a recovery action;
  do not repeatedly add routes.
- Logs must include the stable error code and high-level operation, but not subscription URLs,
  tokens, or sensitive route data.
- TUN health is separate from core health. A healthy Clash API with a broken route must report
  capture failure, not success.
- The first-release bypass policy is fixed: loopback, link-local, multicast, RFC1918, RFC4193,
  the Mixed and Clash API endpoints, the TUN CIDR, DNS resolver traffic, and ice-box/sing-box
  control traffic use the documented direct path. Other LAN destinations follow normal routing
  policy; `allow_lan` does not broaden the bypass list.
- IPv4 support is mandatory for a supported TUN backend. IPv6 is best effort and must be exposed
  as a capability/limitation in status and documentation; the product must not label a
  best-effort setup as "all traffic".

## 8. Rollout and compatibility

1. Land T0-S and the architecture amendment first; this unlocks shared T1 work.
2. Land T1 shared code behind a disabled-by-default capability check while platform gates run.
3. Enable the macOS TUN slices only after `macos_tun_ready`; enable the Windows TUN slices only
   after `windows_tun_ready`.
4. Promote each platform to TUN release readiness only after that platform's acceptance and
   packaging gates pass. For this macOS release, the clean-machine gate is explicitly waived.
   A combined macOS + Windows release still requires the Windows platform gate, but one
   platform may ship or be tested independently during development.
5. Keep system proxy as the documented fallback on platforms whose TUN gate is pending or failed.
6. Do not expand to Linux, mobile, per-app routing, or remote control in the same slice.

Versioning and release notes must call out new host prerequisites (driver, entitlement, UAC,
helper, or in-app elevation) and any behavior change to routes/DNS. Formal releases still follow
`docs/release-process.md`.

## 9. Remaining T0 decisions and locked product answers

The following are the only decisions allowed to remain open after this plan is approved:

1. **sing-box schema and pin:** the shared schema is locked in T0-S; whether the exact Tun config
   is accepted on each host is a per-platform gate. If not, an explicit pin upgrade or platform
   adapter decision is required before enabling TUN on that platform, not before shared T1.
2. **Platform permission mechanism:** native sing-box, a privileged helper (installed through
   the system authorization dialog on the unsigned macOS release), or a
   extension/driver, selected independently per platform. The selected path must have one clear
   owner for each adapter/route/DNS resource.
3. **DNS implementation:** DNS correctness and no-loop behavior are mandatory. T0 chooses
   native sing-box hijack versus a platform DNS operation; IP-only capture without a proven DNS
   policy is not releasable.

The following product answers are already locked and must not re-open during implementation:

- Scope is macOS and Windows only for the first release.
- Mixed remains available as a diagnostic inbound in both `Diagnostic` and `Tun` configs; only
  the TUN inbound is omitted from the TUN-disabled (`Diagnostic`) config.
- The bypass policy is the explicit direct list in §7; arbitrary LAN/private bypass expansion is
  out of scope.
- IPv4 is required. IPv6 is best effort with a visible capability/limitation; unsupported IPv6
  is never presented as complete transparent capture.
- `system_proxy` and TUN are exclusive at the OS boundary, and fallback is permitted only after
  TUN cleanup is verified.

No implementation slice may silently change these answers. T0-S records the shared decisions in
the architecture amendment; each platform gate records its permission, ownership, DNS, and
accepted-field decisions in that platform's acceptance checklist. Shared T1 may complete while
one platform remains unavailable, but no platform's TUN activation is considered complete until
its own gate is green.
