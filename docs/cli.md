# ice-box CLI (`ice-box-cli`)

`ice-box-cli` is the cross-platform headless CLI and daemon for **macOS, Windows, and Linux**. It is built on the shared [`crates/ice-runtime`](../crates/ice-runtime) crate with zero Tauri, GTK, WebKit, or AppKit dependencies, designed for AI Agents, CI pipelines, headless servers, and shell automation.

## One runtime model, two hosts

`ice-box-cli` uses the same authenticated loopback JSON-RPC protocol (`mcp-endpoint.json` + `mcp.secret`) as the desktop [MCP service](mcp.md):

- **With the desktop app running (MCP enabled):** `ice-box-cli` commands talk directly to the live desktop session under the same orchestration lock. Changes appear immediately in the GUI and tray.
- **Without a GUI (headless daemon):** `ice-box-cli daemon` runs the full runtime in the foreground, manages `sing-box` and background watchdogs, and opens the local authenticated control listener automatically.
- **Multiple isolated instances:** pass `--data-dir <PATH>` (or set `ICE_BOX_DATA_DIR`) to run isolated sessions for CI jobs or sandboxed Agents.

## Packages

Each release publishes portable archives containing `ice-box-cli`, the pinned `sing-box` core, bundled GeoIP/GeoSite `.srs` rule-sets, and platform elevation helpers:

| Platform | Archive | Bundled resources |
|---|---|---|
| macOS (arm64) | `ice-box-cli_<version>_darwin_aarch64.tar.gz` | `resources/sing-box`, `resources/ice-helper`, `resources/geoip/*.srs` |
| Windows (x64) | `ice-box-cli_<version>_windows_x86_64.zip` | `resources/sing-box.exe`, `resources/libcronet.dll`, `resources/ice-tun-launcher.exe`, `resources/geoip/*.srs` |
| Linux (x64) | `ice-box-cli_<version>_linux_x86_64.tar.gz` | `resources/sing-box`, `resources/ice-helper`, `resources/geoip/*.srs` |
| Linux (arm64) | `ice-box-cli_<version>_linux_aarch64.tar.gz` | `resources/sing-box`, `resources/ice-helper`, `resources/geoip/*.srs` |

Build a local archive from source with:

```bash
bash scripts/fetch-singbox.sh
bash scripts/fetch-geoip.sh
bash scripts/package-cli.sh
```

`ice-box-cli daemon` automatically resolves `resources/` next to the executable (or override via `--resource-dir` / `ICE_BOX_RESOURCE_DIR`).

## Quick start for Agents and automation

### 1. Start the headless daemon

```bash
ice-box-cli daemon --print-ready
```

`--print-ready` emits one JSON line to `stdout` as soon as the control listener is bound, so supervisors and Agent runners do not need arbitrary sleeps:

```json
{"allow_secrets":true,"data_dir":"/home/user/.local/share/com.yilong-musk.icebox","endpoint":"127.0.0.1:41234","pid":12345,"read_only":false,"ready":true}
```

Useful daemon flags:

| Flag | Purpose |
|---|---|
| `--data-dir <PATH>` | Isolate state, config, logs, and lock file |
| `--resource-dir <PATH>` | Override bundled `sing-box` / `geoip` directory |
| `--no-auto-start` | Start the control listener without launching `sing-box` immediately |
| `--start-service` | Also enable OS capture (system proxy / TUN) after core startup |
| `--read-only` | Allow only read-only tools (`readOnlyHint: true`) on the control listener |
| `--no-secrets` | Block credential-returning tools (`export_subscription`) |
| `--allow-interactive-elevation` | Opt in to OS GUI elevation prompts (disabled by default in headless mode) |
| `--print-ready` | Print a single readiness JSON line on `stdout` |

Graceful shutdown responds to `SIGINT` / `SIGTERM` / `SIGHUP` on macOS and Linux, and console control / session-end events on Windows, releasing system proxy or TUN capture and removing `mcp-endpoint.json` and `mcp.secret`.

### 2. Zero-privilege proxy execution (`env` and `exec`)

In containers, CI runners, and Agent sandboxes you usually do not need system proxy or TUN capture at all:

```bash
# Run a single command through the local Mixed inbound (propagates child exit code)
ice-box-cli exec -- curl -fsSL https://ifconfig.me

# Or export session-scoped proxy variables into the current shell
eval "$(ice-box-cli env)"                       # bash / zsh / sh
ice-box-cli env --shell fish | source           # fish
ice-box-cli env --shell pwsh | Invoke-Expression # PowerShell
```

### 3. Progressive tool discovery and generic RPC

Instead of loading all 40 MCP tool schemas into an Agent context window up front, Agents can discover tools on demand:

```bash
ice-box-cli tools                   # compact JSON list of all 40 tools (works offline)
ice-box-cli schema update_settings  # full JSON schema for one tool (works offline)
ice-box-cli call get_status         # invoke any tool directly over local RPC
ice-box-cli call set_proxy_mode '{"mode":"rule"}'
```

`ice-box-cli mcp` (or `ice-box-cli --mcp-stdio`) also acts as a drop-in MCP stdio bridge targeting either the headless daemon or the desktop app.

## Command reference

Global options: `--data-dir <PATH>`, `--resource-dir <PATH>`, `--json`, `--human`, `--timeout <SECS>`, `-V/--version`, `-h/--help`.

By default, commands emit JSON automatically when `stdout` is not a TTY (pipes, scripts, Agents) and concise human-readable text on an interactive terminal. `env` and `config` stay raw text unless `--json` is passed explicitly.

| Command | Description |
|---|---|
| `daemon` (`serve`) | Run the headless runtime in the foreground |
| `mcp` | Run the authenticated MCP stdio bridge |
| `ping` | Verify the local control service is reachable |
| `wait-ready [--core] [--service]` | Poll until the control listener (and optionally core/capture) is ready |
| `status` | Inspect core, inbound, capture, subscription, and runtime status |
| `start [--wait]` / `stop` | Enable or disable OS proxy/TUN capture |
| `core <start\|stop>` | Start or stop `sing-box` directly without changing capture |
| `mode <rule\|global\|direct>` | Switch routing mode |
| `env [--shell ...]` | Emit session-only proxy environment commands |
| `exec -- <CMD...>` | Run one command with session-only proxy environment variables |
| `sub list\|add\|import\|update\|use\|deactivate\|rm\|auto-update\|export` | Manage subscriptions (`import -` reads stdin) |
| `node list\|use\|group\|test` | List nodes, select outbound/group member, or probe delay |
| `rule overview\|list\|enable\|disable\|add\|update\|rm` | Inspect and manage routing rules |
| `ruleset list\|refresh` | Inspect or refresh cached rule-sets |
| `conn list\|close\|close-all` | Inspect or close live connections |
| `traffic [--since <CURSOR>]` | Read live traffic counters |
| `logs [-n <N>] [--all]` | Read merged application and core logs |
| `config` | Print the redacted runtime `sing-box` configuration |
| `settings get\|set <JSON\|--stdin>` | Read or patch application settings |
| `tun recover\|install-helper\|uninstall-helper\|ensure-elevation\|remove-elevation` | Manage TUN recovery and platform elevation |
| `tools [--live]` / `schema <TOOL>` / `call <TOOL> [JSON]` | Tool catalog, schema lookup, and generic RPC |

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Success |
| `1` | Application or runtime error |
| `2` | CLI usage error or invalid JSON input |
| `3` | Control service / daemon is not running (`cli.not_running`) |
| `4` | Authentication or endpoint config error (`cli.auth_failed`) |
| `5` | Privileged helper / administrator elevation required, or tool blocked by `--read-only` / `--no-secrets` (`mcp.policy_denied`) |
| `6` | RPC or `wait-ready` timeout |

Errors print `{"error":{"code":"...","message":"..."}}` to `stderr` when JSON mode is active.

## Headless elevation and security

- By default, `ice-box-cli daemon` disables interactive GUI elevation prompts (`Polkit` / macOS `AuthorizationServices` / Windows UAC) so unattended scripts and Agents fail fast with exit code `5` and an actionable command (`sudo <path>/ice-helper install ...` on macOS/Linux, or running from an elevated shell on Windows).
- Use `--read-only` and/or `--no-secrets` when granting local CLI or MCP access to untrusted or semi-trusted Agents.
