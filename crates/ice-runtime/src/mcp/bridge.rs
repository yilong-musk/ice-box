// SPDX-License-Identifier: GPL-3.0-or-later

use serde::Deserialize;
use serde_json::Value;
use std::ffi::OsString;
use std::fmt;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

const MCP_ENDPOINT_FILE: &str = "mcp-endpoint.json";
const MCP_SECRET_FILE: &str = "mcp.secret";

#[derive(Deserialize)]
struct EndpointFile {
    host: String,
    port: u16,
    #[allow(dead_code)]
    pid: u32,
    api_version: u32,
}

/// Structured error returned by the local loopback MCP/control client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeError {
    /// Endpoint/secret file is missing or the listener refused the connection.
    NotRunning(String),
    /// Endpoint/secret file was malformed or rejected the control token.
    AuthOrConfig(String),
    /// Network / framing / JSON-RPC protocol error after connecting.
    Transport(String),
    /// Tool returned `isError: true` with an application error code and message.
    ToolError { code: String, message: String },
}

impl BridgeError {
    pub fn code(&self) -> &str {
        match self {
            Self::NotRunning(_) => "cli.not_running",
            Self::AuthOrConfig(_) => "cli.auth_failed",
            Self::Transport(_) => "cli.transport_error",
            Self::ToolError { code, .. } => code.as_str(),
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::NotRunning(m) | Self::AuthOrConfig(m) | Self::Transport(m) => m.as_str(),
            Self::ToolError { message, .. } => message.as_str(),
        }
    }
}

impl fmt::Display for BridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotRunning(m) | Self::AuthOrConfig(m) | Self::Transport(m) => f.write_str(m),
            Self::ToolError { code, message } => write!(f, "{code}: {message}"),
        }
    }
}

impl std::error::Error for BridgeError {}

/// Successful result of invoking an MCP/control tool.
#[derive(Debug, Clone)]
pub struct ToolCallOutput {
    /// Structured JSON payload (`structuredContent` unwrapped when it was boxed
    /// into `{"result": ...}` for non-object values).
    pub value: Value,
    /// Pretty-printed text block returned by the server.
    pub text: String,
}

pub fn run_stdio_bridge(args: impl IntoIterator<Item = OsString>) -> i32 {
    let data_dir = match parse_data_dir(args) {
        Ok(dir) => dir,
        Err(message) => {
            eprintln!("ice-box mcp bridge: {message}");
            return 1;
        }
    };
    match connect(&data_dir)
        .map_err(|err| err.to_string())
        .and_then(proxy_stdio)
    {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("ice-box mcp bridge: {error}");
            1
        }
    }
}

fn parse_data_dir(args: impl IntoIterator<Item = OsString>) -> Result<PathBuf, String> {
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if arg == "--data-dir" {
            return args
                .next()
                .map(PathBuf::from)
                .ok_or_else(|| "--data-dir requires a path".to_string());
        }
    }
    if let Some(dir) = std::env::var_os("ICE_BOX_DATA_DIR") {
        return Ok(PathBuf::from(dir));
    }
    default_data_dir().ok_or_else(|| {
        "could not resolve the ice-box data directory; pass --data-dir or set ICE_BOX_DATA_DIR"
            .to_string()
    })
}

/// Platform default application data directory matching the Tauri desktop shell.
pub fn default_data_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    let result = std::env::var_os("HOME").map(|home| {
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("com.yilong-musk.icebox")
    });
    #[cfg(target_os = "windows")]
    let result =
        std::env::var_os("APPDATA").map(|dir| PathBuf::from(dir).join("com.yilong-musk.icebox"));
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let result = std::env::var_os("XDG_DATA_HOME")
        .map(|dir| PathBuf::from(dir).join("com.yilong-musk.icebox"))
        .or_else(|| {
            std::env::var_os("HOME")
                .map(|home| PathBuf::from(home).join(".local/share/com.yilong-musk.icebox"))
        });
    result
}

/// Open an authenticated loopback connection to the running desktop app or
/// headless daemon for `data_dir`.
pub fn connect(data_dir: &Path) -> Result<TcpStream, BridgeError> {
    let endpoint_path = data_dir.join(MCP_ENDPOINT_FILE);
    let secret_path = data_dir.join(MCP_SECRET_FILE);
    let endpoint_text = std::fs::read_to_string(&endpoint_path).map_err(|error| {
        BridgeError::NotRunning(format!(
            "ice-box control service is not running ({}: {error})",
            endpoint_path.display()
        ))
    })?;
    let endpoint: EndpointFile = serde_json::from_str(&endpoint_text).map_err(|error| {
        BridgeError::AuthOrConfig(format!("invalid {}: {error}", endpoint_path.display()))
    })?;
    if endpoint.api_version != 1 {
        return Err(BridgeError::AuthOrConfig(format!(
            "unsupported MCP bridge API version {}",
            endpoint.api_version
        )));
    }
    let ip: IpAddr = endpoint.host.parse().map_err(|error| {
        BridgeError::AuthOrConfig(format!("invalid MCP endpoint host: {error}"))
    })?;
    if !ip.is_loopback() {
        return Err(BridgeError::AuthOrConfig(
            "MCP endpoint must be loopback-only".into(),
        ));
    }
    let token = std::fs::read_to_string(&secret_path).map_err(|error| {
        BridgeError::NotRunning(format!("MCP control token is unavailable: {error}"))
    })?;
    let mut stream =
        TcpStream::connect_timeout(&SocketAddr::new(ip, endpoint.port), Duration::from_secs(3))
            .map_err(|error| {
                BridgeError::NotRunning(format!("connect to running ice-box service: {error}"))
            })?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| BridgeError::Transport(format!("configure MCP bridge: {error}")))?;
    let handshake = serde_json::to_vec(&serde_json::json!({
        "token": token.trim(),
    }))
    .map_err(|error| BridgeError::Transport(format!("encode MCP handshake: {error}")))?;
    stream
        .write_all(&handshake)
        .and_then(|_| stream.write_all(b"\n"))
        .and_then(|_| stream.flush())
        .map_err(|error| BridgeError::Transport(format!("send MCP handshake: {error}")))?;
    let mut ack = String::new();
    BufReader::new(
        stream
            .try_clone()
            .map_err(|error| BridgeError::Transport(format!("clone MCP connection: {error}")))?,
    )
    .read_line(&mut ack)
    .map_err(|error| BridgeError::Transport(format!("read MCP handshake response: {error}")))?;
    if ack.trim() != r#"{"ok":true}"# {
        return Err(BridgeError::AuthOrConfig(
            "MCP service rejected the local control token".into(),
        ));
    }
    stream
        .set_read_timeout(None)
        .map_err(|error| BridgeError::Transport(format!("configure MCP bridge: {error}")))?;
    Ok(stream)
}

/// Send one JSON-RPC request over an authenticated stream and read its single-line response.
pub fn send_rpc(
    stream: &mut TcpStream,
    request: &Value,
    timeout: Option<Duration>,
) -> Result<Value, BridgeError> {
    stream
        .set_read_timeout(timeout)
        .map_err(|error| BridgeError::Transport(format!("set read timeout: {error}")))?;
    stream
        .set_write_timeout(timeout.or(Some(Duration::from_secs(10))))
        .map_err(|error| BridgeError::Transport(format!("set write timeout: {error}")))?;
    let encoded = serde_json::to_vec(request)
        .map_err(|error| BridgeError::Transport(format!("encode JSON-RPC request: {error}")))?;
    stream
        .write_all(&encoded)
        .and_then(|_| stream.write_all(b"\n"))
        .and_then(|_| stream.flush())
        .map_err(|error| BridgeError::Transport(format!("send JSON-RPC request: {error}")))?;
    let mut line = String::new();
    let mut reader = BufReader::new(
        stream
            .try_clone()
            .map_err(|error| BridgeError::Transport(format!("clone MCP stream: {error}")))?,
    );
    let n = reader
        .read_line(&mut line)
        .map_err(|error| BridgeError::Transport(format!("read JSON-RPC response: {error}")))?;
    if n == 0 {
        return Err(BridgeError::Transport(
            "control service closed the connection before responding".into(),
        ));
    }
    serde_json::from_str(line.trim())
        .map_err(|error| BridgeError::Transport(format!("parse JSON-RPC response: {error}")))
}

/// Verify that the local control service for `data_dir` is reachable and responds to `ping`.
pub fn ping(data_dir: &Path, timeout: Option<Duration>) -> Result<(), BridgeError> {
    let mut stream = connect(data_dir)?;
    let response = send_rpc(
        &mut stream,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "ping",
        }),
        timeout.or(Some(Duration::from_secs(5))),
    )?;
    if let Some(error) = response.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("ping failed");
        return Err(BridgeError::Transport(message.to_string()));
    }
    Ok(())
}

/// Fetch the live tool catalog from a running service.
pub fn list_tools(data_dir: &Path, timeout: Option<Duration>) -> Result<Vec<Value>, BridgeError> {
    let mut stream = connect(data_dir)?;
    let response = send_rpc(
        &mut stream,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list",
        }),
        timeout.or(Some(Duration::from_secs(10))),
    )?;
    if let Some(error) = response.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("tools/list failed");
        return Err(BridgeError::Transport(message.to_string()));
    }
    response
        .pointer("/result/tools")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| BridgeError::Transport("tools/list response missing tools array".into()))
}

/// Invoke one tool on the running desktop app or headless daemon.
pub fn call_tool(
    data_dir: &Path,
    name: &str,
    arguments: Value,
    timeout: Option<Duration>,
) -> Result<ToolCallOutput, BridgeError> {
    let mut stream = connect(data_dir)?;
    let response = send_rpc(
        &mut stream,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": name,
                "arguments": arguments,
            },
        }),
        timeout,
    )?;
    if let Some(error) = response.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("JSON-RPC error");
        return Err(BridgeError::Transport(message.to_string()));
    }
    let result = response
        .get("result")
        .ok_or_else(|| BridgeError::Transport("JSON-RPC response missing result".into()))?;
    let is_error = result
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let text = result
        .pointer("/content/0/text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if is_error {
        let code = result
            .pointer("/structuredContent/error/code")
            .and_then(Value::as_str)
            .unwrap_or("cli.tool_error")
            .to_string();
        let message = result
            .pointer("/structuredContent/error/message")
            .and_then(Value::as_str)
            .unwrap_or(text.as_str())
            .to_string();
        return Err(BridgeError::ToolError { code, message });
    }
    let value = match result.get("structuredContent") {
        Some(Value::Object(map)) if map.len() == 1 && map.contains_key("result") => {
            map.get("result").cloned().unwrap_or(Value::Null)
        }
        Some(other) => other.clone(),
        None => serde_json::from_str(&text).unwrap_or_else(|_| Value::String(text.clone())),
    };
    Ok(ToolCallOutput { value, text })
}

fn proxy_stdio(mut stream: TcpStream) -> Result<(), String> {
    let mut upstream = stream
        .try_clone()
        .map_err(|error| format!("clone MCP connection: {error}"))?;
    let input_thread = thread::spawn(move || -> std::io::Result<()> {
        let stdin = std::io::stdin();
        let mut reader = stdin.lock();
        let mut buffer = [0_u8; 16 * 1024];
        loop {
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            upstream.write_all(&buffer[..read])?;
            upstream.flush()?;
        }
        let _ = upstream.shutdown(std::net::Shutdown::Write);
        Ok(())
    });

    let stdout = std::io::stdout();
    let mut output = stdout.lock();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = stream
            .read(&mut buffer)
            .map_err(|error| format!("read from the desktop app: {error}"))?;
        if read == 0 {
            break;
        }
        output
            .write_all(&buffer[..read])
            .and_then(|_| output.flush())
            .map_err(|error| format!("write MCP response: {error}"))?;
    }
    // A stdio client commonly keeps stdin open while the desktop process exits.
    // Dropping the handle avoids waiting forever on that blocked read; the
    // bridge process exits as soon as this main thread returns.
    drop(input_thread);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dir_argument_wins_over_environment() {
        let dir = parse_data_dir([
            OsString::from("--mcp-stdio"),
            OsString::from("--data-dir"),
            OsString::from("/tmp/ice-box-mcp-test"),
        ])
        .expect("data dir");
        assert_eq!(dir, PathBuf::from("/tmp/ice-box-mcp-test"));
    }

    #[test]
    fn missing_data_dir_argument_is_rejected() {
        let error = parse_data_dir([OsString::from("--mcp-stdio"), OsString::from("--data-dir")])
            .expect_err("missing data dir");
        assert!(error.contains("requires a path"));
    }
}
