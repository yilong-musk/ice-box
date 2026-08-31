# ice-helper — privileged helper daemon (T5)

Status: **T5 design note** (recorded with `docs/tun-mode-plan.md` §5 T5 and the
architecture amendment §24.5.2). The helper daemon, its IPC protocol, the app-side
coordinator, the privileged install/uninstall modes, entitlements, and the live
acceptance mode are landed, and G9.13 has passed on an authenticated macOS host.
The release is **permanently unsigned**: the helper is installed through the system
authorization dialog on first use (in-app, `install_helper` IPC, `crates/ice-elevate`)
or via the install script. The clean-machine install gate is explicitly waived
for the current release.

## 1. Why a helper

macOS T0 lock (§24.5.2): creating a utun, assigning addresses, and adding routes are
**privileged**; the bundled sing-box must run elevated. The locked production execution
context is a small launchd daemon that runs the core as root. Native sing-box owns
the adapter / addresses / routes / DNS; ice-box coordinates and verifies. A
network-extension package is not required.

The helper exists only because elevation cannot come from the unelevated app. Its scope is
therefore exactly the elevated thing the app needs: **start and stop the bundled core with
an allowlisted config path**. Everything else (adapter, routes, DNS, journal, recovery)
stays in `ice-tun-sys`/the app, so the helper's IPC surface is deliberately tiny.

## 2. IPC protocol

Wire: **one JSON object per line** over a Unix stream socket, UTF-8, each frame capped at
16 KiB (`MAX_FRAME_BYTES`). One request and one response per connection; the client
reconnects per command, so the daemon holds no connection state.

Requests (`HelperRequest`, protocol version 1):

```json
{"v": 1, "token": "...", "cmd": "status"}
{"v": 1, "token": "...", "cmd": "start", "config": "/abs/path/config.json"}
{"v": 1, "token": "...", "cmd": "stop"}
```

Responses:

```json
{"ok": true, "pid": 1234}
{"ok": false, "code": "tun.permission_required", "message": "..."}
```

`code` values are the stable `tun.*` codes from `ice_tun_sys::TunErrorCode`. The protocol
types and frame codecs live in `crates/ice-tun-sys/src/helper_protocol.rs`, shared by the
client (`helper.rs`) and the daemon (`crates/ice-helper`).

## 3. Security model (plan §7)

- **Peer identity:** the daemon reads the socket's `getpeereid` (macOS; `SO_PEERCRED` on
  Linux for tests) and requires the peer uid to equal the authorized user the installer
  recorded (`ICE_HELPER_ALLOWED_UID`). Rejected before the frame is read.
- **Token:** each installation gets a random 32-byte hex token (`scripts/install-helper-macos.sh`),
  written root-owned 0644 into the app data dir (`helper-token`) and injected into the
  launchd plist. The daemon compares it constant-time.
- **Narrow command set:** `status` / `start` / `stop` only. No binary path, route target,
  interface name, or shell input is ever accepted from the client.
- **Path allowlist:** `start` config must be absolute and canonicalize to a regular file
  **inside** the installed data dir (`validate_config_path`); `..` and symlink escapes are
  rejected. The core binary path is fixed at install (`ICE_HELPER_CORE_BIN`) — never
  client-supplied.
- **Root-executed binary integrity:** the installer copies the core into a root-owned,
  wheel-only location (`/Library/PrivilegedHelperTools/com.yilong-musk.icebox/sing-box`,
  mode 0755), refuses a group/world-writable source, verifies ownership/mode at install, and
  pins the binary's SHA-256
  in the plist (`ICE_HELPER_CORE_BIN_SHA256`). The daemon refuses to start when the
  on-disk binary does not match the pinned hash, so a user-writable or tampered core can
  never be executed as root.
- **Socket connectable, connection authenticated:** `/var/run/ice-box-helper.sock` is
  world-connectable (mode 0666) because the desktop app runs as the normal user while the
  daemon runs as root. Authorization happens *on top* of the connection — peer uid
  (`getpeereid`) and the per-installation token — so an unauthenticated peer gets nothing.

## 4. Components

| Piece | Location | Notes |
|-------|----------|-------|
| Protocol types + framing + path validation | `crates/ice-tun-sys/src/helper_protocol.rs` | Host-free, tests on all CI platforms |
| App-side client (`HelperCoreCoordinator`) | `crates/ice-tun-sys/src/helper.rs` | Implements `CoreCoordinator`; per-command reconnect; bounded timeouts; token from env or `helper-token` file |
| Daemon server (`serve_connection`, `ProcessCoreRunner`) | `crates/ice-helper/src/lib.rs` | Peer auth, dispatch, TERM→KILL with bounded grace, core reaping |
| Daemon entry (`main.rs`) | `crates/ice-helper/src/main.rs` | Binds socket, accept loop; env-driven config; non-unix stub for CI |
| Backend wiring | `crates/ice-tun-sys/src/lib.rs` `create_backend` | macOS: dev `sudo` opt-in wins, else helper when authorized (read-only `status` probe), else fail-closed deferred |
| Install / uninstall | `ice-helper install|uninstall` privileged modes (`crates/ice-helper/src/install.rs`), driven by the in-app authorization dialog (`crates/ice-elevate` + `install_helper`/`uninstall_helper` IPC) or the scripts | launchctl bootstrap/bootout, token generation, plist with pinned env |
| Entitlements | `apps/desktop/src-tauri/entitlements/ice-helper.entitlements` | Helper is a plain daemon; only hardened-runtime toggles |
| Live acceptance | `scripts/run-acceptance-macos-tun.sh --helper` + G9.13 | Install → enable → mixed curl → disable → adapter-removed → uninstall |

The helper's `stop` mirrors `SudoCoreCoordinator`'s bounded TERM→KILL and keeps the
`Child` handle so the process is **reaped** (a zombie would stay "alive" to `kill(pid, 0)`).
`status` and the next `start` also poll `try_wait`: a core that exited on its own is reaped
immediately, so the daemon never reports a stale pid and never rejects a new `start`
because of an exited core.

## 5. Coordinator selection on macOS

`create_backend` picks the elevated-core runner in this order:

1. `ICE_BOX_TUN_DEV_SUDO` set → `SudoCoreCoordinator` (dev live gate, unchanged).
2. Helper socket reachable + token valid (`status` roundtrip) → `HelperCoreCoordinator`.
3. Otherwise → `DeferredCoreCoordinator` (every transition fails cleanly with
   `tun.permission_required`; no OS mutation).

The probe is read-only and bounded (3 s timeouts); a missing helper keeps the app fully
functional on the system-proxy path.

## 6. Install / uninstall flow

There is exactly one implementation of the installation logic, in the helper
binary itself (`crates/ice-helper/src/install.rs`). Two drivers run it as root:

- **In-app (default UX):** the `install_helper` / `uninstall_helper` IPC commands prompt the
  system authorization dialog via `AuthorizationServices`
  (`crates/ice-elevate`; deprecated-but-functional, chosen because SMAppService
  requires code signing and the app is permanently unsigned) and execute
  `ice-helper install <data-dir> <core-src> <allowed-uid>` / `ice-helper uninstall <data-dir>`
  as root. The desktop process stays unelevated; cancelling the dialog modifies nothing.
- **Script (manual / CI):** `scripts/install-helper-macos.sh [DATA_DIR] [CORE_BIN] [HELPER_BIN]`
  and `scripts/uninstall-helper-macos.sh [DATA_DIR]` run the same modes through `sudo`
  (builds the helper locally when no binary is supplied).

The install mode, running as root:

1. Verifies `euid == 0`; refuses otherwise.
2. Refuses a core source that is not a regular file or is group/world-writable.
3. Generates the per-installation token (from `/dev/urandom`), writes
   `$DATA_DIR/helper-token` (root:wheel 0644).
4. Copies itself (the running executable) to
   `/Library/PrivilegedHelperTools/com.yilong-musk.icebox.helper`.
5. Copies the core binary to the root-owned
   `/Library/PrivilegedHelperTools/com.yilong-musk.icebox/sing-box`
   (root:wheel 0755) and pins its SHA-256.
6. Recreates the fixed root-owned log files `/var/log/ice-box-core.log` and
   `/var/log/ice-box-helper.log` (a stale symlink can never become the target
   of a privileged append; the daemon opens the core log with `O_NOFOLLOW`).
7. Writes `/Library/LaunchDaemons/com.yilong-musk.icebox.helper.plist` pinning
   `ICE_HELPER_TOKEN` / `ICE_HELPER_DATA_DIR` / `ICE_HELPER_CORE_BIN` /
   `ICE_HELPER_CORE_BIN_SHA256` / `ICE_HELPER_CORE_LOG` /
   `ICE_HELPER_ALLOWED_UID` / `ICE_BOX_TUN_HELPER_SOCKET`.
8. `launchctl bootstrap system` (bootout of a previous job is ignored).

The uninstall mode: `launchctl bootout`, removes binary, plist, the fixed
helper logs, socket, and token file. Never touches routes/adapters/DNS.

The helper prints a single result line (`OK ...` / `ERROR: ...`); the app
parses it because the AuthorizationServices pipe does not expose the tool's
exit code.

## 7. Release pipeline

- The bundle embeds the helper at `Contents/Resources/ice-helper`
  (`scripts/prepare-singbox-resource.sh` + `tauri.conf.json` resources; CI verifies it).
- The release is **permanently unsigned** (product decision): Developer ID
  signing and notarization are not part of the product, SMAppService is
  intentionally not used, and helper installation happens through the system
  authorization dialog on first use. Gatekeeper warnings are expected on
  published artifacts.
- The clean-machine install/uninstall gate is explicitly waived for the current macOS release
  and is not a release blocker. G9.13 still performs the destructive install → enable → disable
  → uninstall sequence on an authenticated host and verifies helper filesystem/launchd cleanup
  even on failure.
