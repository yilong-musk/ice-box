// SPDX-License-Identifier: GPL-3.0-or-later

use serde::Deserialize;
use std::ffi::OsString;
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

pub(crate) fn run_stdio_bridge(args: impl IntoIterator<Item = OsString>) -> i32 {
    let data_dir = match parse_data_dir(args) {
        Ok(dir) => dir,
        Err(message) => {
            eprintln!("ice-box mcp bridge: {message}");
            return 1;
        }
    };
    match connect(&data_dir).and_then(proxy_stdio) {
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

fn default_data_dir() -> Option<PathBuf> {
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

fn connect(data_dir: &Path) -> Result<TcpStream, String> {
    let endpoint_path = data_dir.join(MCP_ENDPOINT_FILE);
    let secret_path = data_dir.join(MCP_SECRET_FILE);
    let endpoint_text = std::fs::read_to_string(&endpoint_path).map_err(|error| {
        format!(
            "MCP service is not running ({}: {error})",
            endpoint_path.display()
        )
    })?;
    let endpoint: EndpointFile = serde_json::from_str(&endpoint_text)
        .map_err(|error| format!("invalid {}: {error}", endpoint_path.display()))?;
    if endpoint.api_version != 1 {
        return Err(format!(
            "unsupported MCP bridge API version {}",
            endpoint.api_version
        ));
    }
    let ip: IpAddr = endpoint
        .host
        .parse()
        .map_err(|error| format!("invalid MCP endpoint host: {error}"))?;
    if !ip.is_loopback() {
        return Err("MCP endpoint must be loopback-only".into());
    }
    let token = std::fs::read_to_string(&secret_path)
        .map_err(|error| format!("MCP control token is unavailable: {error}"))?;
    let mut stream =
        TcpStream::connect_timeout(&SocketAddr::new(ip, endpoint.port), Duration::from_secs(3))
            .map_err(|error| format!("connect to the running desktop app: {error}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| format!("configure MCP bridge: {error}"))?;
    let handshake = serde_json::to_vec(&serde_json::json!({
        "token": token.trim(),
    }))
    .map_err(|error| format!("encode MCP handshake: {error}"))?;
    stream
        .write_all(&handshake)
        .and_then(|_| stream.write_all(b"\n"))
        .and_then(|_| stream.flush())
        .map_err(|error| format!("send MCP handshake: {error}"))?;
    let mut ack = String::new();
    BufReader::new(
        stream
            .try_clone()
            .map_err(|error| format!("clone MCP connection: {error}"))?,
    )
    .read_line(&mut ack)
    .map_err(|error| format!("read MCP handshake response: {error}"))?;
    if ack.trim() != r#"{"ok":true}"# {
        return Err("MCP service rejected the local control token".into());
    }
    stream
        .set_read_timeout(None)
        .map_err(|error| format!("configure MCP bridge: {error}"))?;
    Ok(stream)
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
