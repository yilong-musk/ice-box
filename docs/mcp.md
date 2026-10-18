# MCP for the Desktop App

ice-box exposes a local [Model Context Protocol](https://modelcontextprotocol.io/)
endpoint so an Agent can inspect and operate the same desktop session that a
person is using.

MCP is a desktop-only capability. The macOS and Windows builds include it. The
Android app and the website Live Demo do not start an MCP service and do not
show an MCP page.

## One runtime, two control surfaces

The desktop application remains the only runtime owner:

```text
Person -> React GUI --------+
                           v
Agent -> MCP stdio bridge -> desktop MCP listener
                           |
                           v
              AppState, application use cases,
              data directory, core, and OS capture
```

The Agent does not receive a second `AppState`, a headless daemon, or another
data directory. GUI actions and MCP tool calls enter the same application use
cases, use the same orchestration lock, and update the same runtime state. A
change made by either control surface is visible to the other.

The stdio bridge is not a second application source. It is a lightweight mode
of the same desktop executable that forwards MCP JSON-RPC to the listener in
the running application. It exits with the Agent's stdio session.

## Enable and connect

1. Start ice-box and open **MCP** in the desktop navigation. From top to bottom,
   the final entries are **MCP**, **Logs**, and **Settings**.
2. Enable **MCP Service**.
3. Copy the generated client configuration.
4. Add that configuration to an MCP-capable Agent client and start or restart
   the client.

The service is opt-in. Its setting is stored in `mcp.json` in the application
data directory and is off until a person enables it.

The generated configuration has this shape:

```json
{
  "mcpServers": {
    "ice-box": {
      "command": "/Applications/ice-box.app/Contents/MacOS/ice-box",
      "args": [
        "--mcp-stdio",
        "--data-dir",
        "/Users/example/Library/Application Support/com.yilong-musk.icebox"
      ]
    }
  }
}
```

Use the configuration copied from the page. The executable path and data
directory are resolved for the current installation. In particular, keep the
data directory unchanged: the bridge uses it to locate the running desktop
service.

The MCP page reports:

- whether the service is enabled and currently running;
- the random loopback endpoint;
- connected Agent client count and available tool count;
- the ready-to-paste bridge configuration.

The client count covers authenticated bridge sessions. A bridge can have only
one Agent stdio pipe, although several Agents may connect to the same desktop
service at once.

## Lifecycle

Closing the window normally leaves ice-box running in the tray, so the MCP
service remains available. Quitting ice-box stops the listener and closes all
bridge connections.

Each start or restart selects a new random loopback port and control token. The
desktop process rewrites `mcp-endpoint.json` and `mcp.secret`; the bridge reads
them for each new connection. A client should therefore start the bridge
through the configured command rather than caching the endpoint port.

Disabling or restarting the service waits for active tool calls to finish
before reporting success. At most 32 bridge connections may be open at once.

If graceful shutdown fails and the desktop app stays open, the MCP service
also stays open. This lets the person or Agent inspect the failure and retry
instead of losing the control surface while the runtime is still active.

## Tools

The MCP server currently exposes 34 tools through the application services:

- Status and settings: `get_status`, `get_settings`, `update_settings`.
- Core and capture: `start_proxy_service`, `stop_proxy_service`, `start_core`,
  `stop_core`, and `set_proxy_mode`.
- Traffic: `get_traffic_snapshot`, `get_traffic_since`.
- Subscriptions: `list_subscriptions`, `add_subscription`,
  `import_subscription_document`, `remove_subscription`,
  `update_subscription`, `set_subscription_active`,
  `set_subscription_auto_update`, and `export_subscription`.
- Nodes and groups: `list_nodes`, `select_node`, `select_group_member`, and
  `test_node_delay`.
- Rules: `get_rule_overview`, `list_rules`, `set_rule_disabled`,
  `add_custom_rule`, and `remove_custom_rule`.
- Diagnostics: `get_logs` and `get_runtime_config`.
- Protected capture recovery: `recover_tun`, `install_tun_helper`,
  `uninstall_tun_helper`, `ensure_tun_elevation`, and
  `remove_tun_elevation`.

Results use MCP text content plus structured content when the result is an
object. Tool failures set `isError` and return the application error code and
message rather than claiming success.

`update_settings` rejects `proxy_service_enabled`; use
`start_proxy_service` or `stop_proxy_service` so the live capture backend and
the persisted next-launch preference change together. A successful
`stop_core` also records the proxy service off before stopping the core.

Tools declare MCP annotations. Read-only tools are marked with
`readOnlyHint`. Tools that can change local state are marked with
`destructiveHint`; this is a signal to the Agent client, not an additional
confirmation dialog.

## Security boundary

The listener accepts only loopback connections and requires a random token
written to `mcp.secret`. On Unix, the file is created with mode `0600`. On
Windows it relies on the per-user application-data directory ACL. The token is
never included in the client configuration shown or copied by the MCP page.
Authentication accepts at most 4 KiB and must complete within five seconds.
Each later JSON-RPC request is limited to 8 MiB before it is parsed.

MCP access is equivalent to local control of the desktop application:

- Subscription and settings tools can persist changes.
- Core and capture tools can change how this computer reaches the network.
- TUN helper actions may show an operating-system authorization prompt.
- `export_subscription` can return subscription credentials.
- `get_runtime_config` redacts known secrets, but logs and other results may
  still contain sensitive operational detail.

Only enable MCP for Agent clients you trust. There is no remote MCP endpoint and
no separate per-tool permission system. The operating system still controls
privileged helper installation or elevation.

## Files

The desktop app keeps MCP state in its normal data directory:

| File | Purpose |
|------|---------|
| `mcp.json` | Persistent enabled/disabled setting |
| `mcp-endpoint.json` | Current loopback host, port, desktop PID, and bridge API version |
| `mcp.secret` | Ephemeral control token for the current service generation |

Endpoint and secret files exist only while the service is running. They are
removed when the service is disabled, restarted, stopped, or exits.

## Troubleshooting

- **The bridge reports that the MCP service is not running:** start ice-box and
  enable MCP on its MCP page. If ice-box is running, restart the service there.
- **An Agent disconnects after an ice-box restart:** restart the Agent's MCP
  server. The endpoint and token intentionally changed.
- **The page shows a configuration error:** repair or remove `mcp.json`, then
  enable the service again. A malformed file fails closed.
- **No MCP entry appears:** you are using Android or the website. MCP is
  intentionally absent on those shells.
- **A protected TUN action stops at an OS prompt:** complete or cancel the
  prompt in the operating system. MCP does not bypass platform authorization.
