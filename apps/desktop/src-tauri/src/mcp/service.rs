// SPDX-License-Identifier: GPL-3.0-or-later

use super::protocol;
use crate::lock_poisoned;
use ice_config::{AppError, ErrorCode};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tauri::AppHandle;

const MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;
const MAX_AUTH_BYTES: usize = 4096;
const MAX_CLIENT_WORKERS: usize = 32;
const ACCEPT_POLL: Duration = Duration::from_millis(50);
const CLIENT_POLL: Duration = Duration::from_millis(250);
const AUTH_TIMEOUT: Duration = Duration::from_secs(5);
const MCP_CONFIG_FILE: &str = "mcp.json";
const MCP_ENDPOINT_FILE: &str = "mcp-endpoint.json";
const MCP_SECRET_FILE: &str = "mcp.secret";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct McpStatus {
    pub supported: bool,
    pub enabled: bool,
    pub running: bool,
    pub endpoint: Option<String>,
    pub client_count: usize,
    pub tool_count: usize,
    pub bridge_executable: String,
    pub bridge_args: Vec<String>,
    pub client_config: String,
    pub error: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct McpConfig {
    #[serde(default)]
    enabled: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct EndpointFile {
    host: String,
    port: u16,
    pid: u32,
    api_version: u32,
}

#[derive(Default)]
struct Inner {
    enabled: bool,
    running: Option<RunningServer>,
    last_error: Option<String>,
}

struct RunningServer {
    endpoint: String,
    stop: Arc<AtomicBool>,
    clients: Arc<ClientRegistry>,
    join: JoinHandle<()>,
}

#[derive(Default)]
struct ClientRegistry {
    authenticated: AtomicUsize,
    open_workers: AtomicUsize,
    next_id: AtomicU64,
    sockets: Mutex<HashMap<u64, TcpStream>>,
    workers: Mutex<Vec<JoinHandle<()>>>,
}

fn reserve_worker_slot(open_workers: &AtomicUsize) -> bool {
    let active = open_workers.fetch_add(1, Ordering::SeqCst);
    if active >= MAX_CLIENT_WORKERS {
        open_workers.fetch_sub(1, Ordering::SeqCst);
        false
    } else {
        true
    }
}

impl ClientRegistry {
    fn register(&self, stream: &TcpStream) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        if let (Ok(socket), Ok(mut sockets)) = (stream.try_clone(), self.sockets.lock()) {
            sockets.insert(id, socket);
        }
        id
    }

    fn unregister(&self, id: u64) {
        if let Ok(mut sockets) = self.sockets.lock() {
            sockets.remove(&id);
        }
    }

    fn add_worker(&self, worker: JoinHandle<()>) {
        if let Ok(mut workers) = self.workers.lock() {
            workers.retain(|worker| !worker.is_finished());
            workers.push(worker);
        }
    }

    fn shutdown_connections(&self) {
        if let Ok(sockets) = self.sockets.lock() {
            for socket in sockets.values() {
                let _ = socket.shutdown(std::net::Shutdown::Both);
            }
        }
    }

    fn finish_workers(&self, wait: bool) {
        let workers = self
            .workers
            .lock()
            .map(|mut workers| std::mem::take(&mut *workers))
            .unwrap_or_default();
        if wait {
            for worker in workers {
                let _ = worker.join();
            }
            self.authenticated.store(0, Ordering::SeqCst);
        }
    }
}

struct ClientConnectionGuard {
    id: u64,
    clients: Arc<ClientRegistry>,
}

impl ClientConnectionGuard {
    fn new(id: u64, clients: Arc<ClientRegistry>) -> Self {
        Self { id, clients }
    }
}

impl Drop for ClientConnectionGuard {
    fn drop(&mut self) {
        self.clients.unregister(self.id);
        self.clients.open_workers.fetch_sub(1, Ordering::SeqCst);
    }
}

struct AuthenticatedClientGuard(Arc<ClientRegistry>);

impl Drop for AuthenticatedClientGuard {
    fn drop(&mut self) {
        self.0.authenticated.fetch_sub(1, Ordering::SeqCst);
    }
}

pub struct McpService {
    data_dir: PathBuf,
    config_path: PathBuf,
    endpoint_path: PathBuf,
    secret_path: PathBuf,
    inner: Mutex<Inner>,
}

impl McpService {
    pub fn new(data_dir: PathBuf) -> Self {
        let config_path = data_dir.join(MCP_CONFIG_FILE);
        let endpoint_path = data_dir.join(MCP_ENDPOINT_FILE);
        let secret_path = data_dir.join(MCP_SECRET_FILE);
        remove_runtime_files(&endpoint_path, &secret_path);
        let (enabled, last_error) = match load_config(&config_path) {
            Ok(config) => (config.enabled, None),
            Err(error) => (false, Some(error.to_string())),
        };
        Self {
            endpoint_path,
            secret_path,
            data_dir,
            config_path,
            inner: Mutex::new(Inner {
                enabled,
                running: None,
                last_error,
            }),
        }
    }

    pub fn start_if_enabled(&self, app: &AppHandle) {
        let enabled = self
            .inner
            .lock()
            .map(|inner| inner.enabled)
            .unwrap_or(false);
        if !enabled {
            return;
        }
        if let Err(error) = self.start(app) {
            if let Ok(mut inner) = self.inner.lock() {
                inner.last_error = Some(error.to_string());
            }
            tracing::warn!(%error, "MCP service failed to start at launch");
        }
    }

    pub fn status(&self) -> McpStatus {
        let (enabled, running, error) = match self.inner.lock() {
            Ok(mut inner) => {
                reconcile_finished_server(&mut inner, &self.endpoint_path, &self.secret_path);
                let running = inner.running.as_ref().map(|server| {
                    (
                        server.endpoint.clone(),
                        server.clients.authenticated.load(Ordering::Relaxed),
                    )
                });
                (inner.enabled, running, inner.last_error.clone())
            }
            Err(_) => (
                false,
                None,
                Some("MCP service state is unavailable because its lock was poisoned".into()),
            ),
        };
        let data_dir = self.data_dir.to_string_lossy().into_owned();
        let bridge_executable = std::env::current_exe()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let bridge_args = vec![
            "--mcp-stdio".to_string(),
            "--data-dir".to_string(),
            data_dir,
        ];
        let client_config = serde_json::to_string_pretty(&serde_json::json!({
            "mcpServers": {
                "ice-box": {
                    "command": bridge_executable,
                    "args": bridge_args,
                }
            }
        }))
        .unwrap_or_else(|_| "{}".to_string());
        McpStatus {
            supported: true,
            enabled,
            running: running.is_some(),
            endpoint: running.as_ref().map(|(endpoint, _)| endpoint.clone()),
            client_count: running.as_ref().map_or(0, |(_, clients)| *clients),
            tool_count: protocol::tools().len(),
            bridge_executable,
            bridge_args,
            client_config,
            error,
        }
    }

    pub fn set_enabled(&self, app: &AppHandle, enabled: bool) -> Result<McpStatus, AppError> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| lock_poisoned("mcp service"))?;
        if enabled {
            if let Err(error) = self.start_locked(app, &mut inner) {
                inner.last_error = Some(error.to_string());
                return Err(error);
            }
            if let Err(error) = save_config(&self.config_path, &McpConfig { enabled: true }) {
                self.stop_locked(&mut inner, true);
                let error = AppError::new(
                    ErrorCode::ConfigInvalid,
                    format!("persist MCP service setting: {error}"),
                );
                inner.last_error = Some(error.to_string());
                return Err(error);
            }
            inner.enabled = true;
        } else {
            if let Err(error) = save_config(&self.config_path, &McpConfig { enabled: false }) {
                let error = AppError::new(
                    ErrorCode::ConfigInvalid,
                    format!("persist MCP service setting: {error}"),
                );
                inner.last_error = Some(error.to_string());
                return Err(error);
            }
            self.stop_locked(&mut inner, true);
            inner.enabled = false;
            inner.last_error = None;
        }
        drop(inner);
        Ok(self.status())
    }

    pub fn restart(&self, app: &AppHandle) -> Result<McpStatus, AppError> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| lock_poisoned("mcp service"))?;
        if !inner.enabled {
            return Err(AppError::new(
                ErrorCode::ConfigInvalid,
                "enable the MCP service before restarting it",
            ));
        }
        self.stop_locked(&mut inner, true);
        if let Err(error) = self.start_locked(app, &mut inner) {
            inner.last_error = Some(error.to_string());
            return Err(error);
        }
        drop(inner);
        Ok(self.status())
    }

    pub fn shutdown(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            self.stop_locked(&mut inner, false);
        }
    }

    fn start(&self, app: &AppHandle) -> Result<(), AppError> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| lock_poisoned("mcp service"))?;
        self.start_locked(app, &mut inner)
    }

    fn start_locked(&self, app: &AppHandle, inner: &mut Inner) -> Result<(), AppError> {
        if inner
            .running
            .as_ref()
            .is_some_and(|server| !server.join.is_finished())
        {
            return Ok(());
        }
        inner.running = None;
        remove_runtime_files(&self.endpoint_path, &self.secret_path);

        let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|error| {
            AppError::new(
                ErrorCode::CoreApiFailed,
                format!("bind MCP loopback listener: {error}"),
            )
        })?;
        listener.set_nonblocking(true).map_err(|error| {
            AppError::new(
                ErrorCode::CoreApiFailed,
                format!("configure MCP loopback listener: {error}"),
            )
        })?;
        let address = listener.local_addr().map_err(|error| {
            AppError::new(
                ErrorCode::CoreApiFailed,
                format!("read MCP listener address: {error}"),
            )
        })?;
        let endpoint = address.to_string();
        let token = uuid::Uuid::new_v4().simple().to_string();
        write_private_file(&self.secret_path, token.as_bytes()).map_err(|error| {
            AppError::new(
                ErrorCode::CoreApiFailed,
                format!("write MCP control token: {error}"),
            )
        })?;
        let endpoint_file = EndpointFile {
            host: address.ip().to_string(),
            port: address.port(),
            pid: std::process::id(),
            api_version: 1,
        };
        if let Err(error) = ice_config::write_json_atomic(&self.endpoint_path, &endpoint_file) {
            let _ = std::fs::remove_file(&self.secret_path);
            return Err(AppError::new(
                ErrorCode::CoreApiFailed,
                format!("write MCP endpoint description: {error}"),
            ));
        }

        let stop = Arc::new(AtomicBool::new(false));
        let clients = Arc::new(ClientRegistry::default());
        let app = app.clone();
        let thread_stop = Arc::clone(&stop);
        let thread_clients = Arc::clone(&clients);
        let join = match thread::Builder::new()
            .name("ice-box-mcp".into())
            .spawn(move || accept_loop(listener, app, token, thread_stop, thread_clients))
        {
            Ok(join) => join,
            Err(error) => {
                remove_runtime_files(&self.endpoint_path, &self.secret_path);
                return Err(AppError::new(
                    ErrorCode::CoreApiFailed,
                    format!("spawn MCP listener: {error}"),
                ));
            }
        };
        inner.running = Some(RunningServer {
            endpoint,
            stop,
            clients,
            join,
        });
        inner.last_error = None;
        Ok(())
    }

    fn stop_locked(&self, inner: &mut Inner, wait_for_clients: bool) {
        if let Some(server) = inner.running.take() {
            server.stop.store(true, Ordering::SeqCst);
            let _ = server.join.join();
            server.clients.shutdown_connections();
            server.clients.finish_workers(wait_for_clients);
        }
        remove_runtime_files(&self.endpoint_path, &self.secret_path);
    }
}

impl Drop for McpService {
    fn drop(&mut self) {
        if let Ok(mut inner) = self.inner.lock() {
            self.stop_locked(&mut inner, false);
        }
    }
}

fn accept_loop(
    listener: TcpListener,
    app: AppHandle,
    token: String,
    stop: Arc<AtomicBool>,
    clients: Arc<ClientRegistry>,
) {
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                if !reserve_worker_slot(&clients.open_workers) {
                    let _ = stream.shutdown(std::net::Shutdown::Both);
                    continue;
                }
                let app = app.clone();
                let token = token.clone();
                let stop = Arc::clone(&stop);
                let id = clients.register(&stream);
                let thread_clients = Arc::clone(&clients);
                let guard_clients = Arc::clone(&clients);
                let worker = thread::Builder::new()
                    .name("ice-box-mcp-client".into())
                    .spawn(move || {
                        let _connection = ClientConnectionGuard::new(id, guard_clients);
                        serve_client(stream, app, token, stop, thread_clients);
                    });
                match worker {
                    Ok(worker) => clients.add_worker(worker),
                    Err(error) => {
                        clients.open_workers.fetch_sub(1, Ordering::SeqCst);
                        clients.unregister(id);
                        tracing::warn!(%error, "spawn MCP client worker");
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_POLL);
            }
            Err(error) => {
                tracing::warn!(%error, "MCP listener stopped");
                break;
            }
        }
    }
}

fn serve_client(
    mut stream: TcpStream,
    app: AppHandle,
    token: String,
    stop: Arc<AtomicBool>,
    clients: Arc<ClientRegistry>,
) {
    if stream.set_nonblocking(false).is_err() {
        return;
    }
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let reader_stream = match stream.try_clone() {
        Ok(stream) => stream,
        Err(_) => return,
    };
    let mut reader = BufReader::new(reader_stream);
    if !authorize_connection(&mut stream, &mut reader, &token, &stop) {
        return;
    }
    clients.authenticated.fetch_add(1, Ordering::SeqCst);
    let _client_guard = AuthenticatedClientGuard(Arc::clone(&clients));
    let _ = stream.set_read_timeout(Some(CLIENT_POLL));
    let mut line = Vec::new();

    loop {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        match read_line_bounded(&mut reader, &mut line, MAX_MESSAGE_BYTES, None, &stop) {
            Ok(BoundedLine::Complete) => {
                let text = match std::str::from_utf8(&line) {
                    Ok(text) => text.trim(),
                    Err(_) => {
                        let response =
                            protocol::protocol_error_value(None, -32700, "request is not UTF-8");
                        if write_json_line(&mut stream, &response).is_err() {
                            break;
                        }
                        line.clear();
                        continue;
                    }
                };
                if let Some(response) = protocol::respond(&app, text) {
                    if write_json_line(&mut stream, &response).is_err() {
                        break;
                    }
                }
                line.clear();
            }
            Ok(BoundedLine::TooLong) => {
                let _ = write_json_line(
                    &mut stream,
                    &protocol::protocol_error_value(None, -32600, "request exceeds 8 MiB"),
                );
                break;
            }
            Ok(BoundedLine::Eof | BoundedLine::TimedOut | BoundedLine::Stopped) => break,
            Err(_) => break,
        }
    }
}

fn authorize_connection(
    stream: &mut TcpStream,
    reader: &mut BufReader<TcpStream>,
    token: &str,
    stop: &AtomicBool,
) -> bool {
    let deadline = Instant::now() + AUTH_TIMEOUT;
    let mut line = Vec::new();
    let complete = matches!(
        read_line_bounded(reader, &mut line, MAX_AUTH_BYTES, Some(deadline), stop),
        Ok(BoundedLine::Complete)
    );
    if !complete {
        return false;
    }
    let allowed = std::str::from_utf8(&line)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(text.trim()).ok())
        .and_then(|value| value.get("token")?.as_str().map(str::to_owned))
        .is_some_and(|candidate| candidate == token);
    if allowed {
        stream.write_all(b"{\"ok\":true}\n").is_ok() && stream.flush().is_ok()
    } else {
        let _ = stream.write_all(b"{\"ok\":false}\n");
        false
    }
}

enum BoundedLine {
    Complete,
    Eof,
    TooLong,
    TimedOut,
    Stopped,
}

fn read_line_bounded(
    reader: &mut BufReader<TcpStream>,
    line: &mut Vec<u8>,
    max_bytes: usize,
    deadline: Option<Instant>,
    stop: &AtomicBool,
) -> std::io::Result<BoundedLine> {
    loop {
        if stop.load(Ordering::SeqCst) {
            return Ok(BoundedLine::Stopped);
        }
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Ok(BoundedLine::TimedOut);
        }

        let chunk = match reader.fill_buf() {
            Ok(chunk) => chunk,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::Interrupted
                ) =>
            {
                continue;
            }
            Err(error) => return Err(error),
        };
        if chunk.is_empty() {
            return Ok(BoundedLine::Eof);
        }

        if let Some(newline) = chunk.iter().position(|byte| *byte == b'\n') {
            if line.len().saturating_add(newline) > max_bytes {
                return Ok(BoundedLine::TooLong);
            }
            line.extend_from_slice(&chunk[..=newline]);
            reader.consume(newline + 1);
            return Ok(BoundedLine::Complete);
        }

        if line.len().saturating_add(chunk.len()) > max_bytes {
            return Ok(BoundedLine::TooLong);
        }
        let consumed = chunk.len();
        line.extend_from_slice(chunk);
        reader.consume(consumed);
    }
}

fn write_json_line(stream: &mut TcpStream, value: &serde_json::Value) -> std::io::Result<()> {
    serde_json::to_writer(&mut *stream, value)?;
    stream.write_all(b"\n")?;
    stream.flush()
}

fn load_config(path: &Path) -> Result<McpConfig, AppError> {
    if !path.exists() {
        return Ok(McpConfig::default());
    }
    let bytes = std::fs::read(path).map_err(|error| {
        AppError::new(
            ErrorCode::ConfigInvalid,
            format!("read {}: {error}", path.display()),
        )
    })?;
    serde_json::from_slice(&bytes).map_err(|error| {
        AppError::new(
            ErrorCode::ConfigInvalid,
            format!("parse {}: {error}", path.display()),
        )
    })
}

fn save_config(path: &Path, config: &McpConfig) -> Result<(), ice_config::ConfigError> {
    ice_config::write_json_atomic(path, config)
}

fn write_private_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::fs::OpenOptions;
    let mut options = OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn remove_runtime_files(endpoint: &Path, secret: &Path) {
    let _ = std::fs::remove_file(endpoint);
    let _ = std::fs::remove_file(secret);
}

fn reconcile_finished_server(inner: &mut Inner, endpoint: &Path, secret: &Path) {
    if !inner
        .running
        .as_ref()
        .is_some_and(|server| server.join.is_finished())
    {
        return;
    }
    if let Some(server) = inner.running.take() {
        let _ = server.join.join();
        server.clients.shutdown_connections();
        server.clients.finish_workers(false);
    }
    remove_runtime_files(endpoint, secret);
    if inner.enabled && inner.last_error.is_none() {
        inner.last_error = Some("MCP listener stopped unexpectedly".into());
    }
}

#[cfg(test)]
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ice-box-mcp-{label}-{}", now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn config_is_opt_in_and_round_trips() {
        let dir = temp_dir("config");
        let path = dir.join(MCP_CONFIG_FILE);
        let service = McpService::new(dir.clone());
        assert!(!service.inner.lock().unwrap().enabled);

        save_config(&path, &McpConfig { enabled: true }).unwrap();
        let loaded = McpService::new(dir.clone());
        assert!(loaded.inner.lock().unwrap().enabled);
        assert_eq!(loaded.secret_path, dir.join(MCP_SECRET_FILE));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn private_file_is_owner_only_on_unix() {
        let dir = temp_dir("secret");
        let path = dir.join(MCP_SECRET_FILE);
        write_private_file(&path, b"token").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"token");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn malformed_config_disables_the_service_and_surfaces_the_error() {
        let dir = temp_dir("malformed");
        std::fs::write(dir.join(MCP_CONFIG_FILE), "{not-json").unwrap();
        let service = McpService::new(dir.clone());
        let status = service.status();
        assert!(!status.enabled);
        assert!(!status.running);
        assert!(status
            .error
            .as_deref()
            .is_some_and(|error| error.contains("parse")));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn construction_removes_stale_runtime_files_from_a_crash() {
        let dir = temp_dir("stale-runtime");
        std::fs::write(dir.join(MCP_ENDPOINT_FILE), b"{}").unwrap();
        std::fs::write(dir.join(MCP_SECRET_FILE), b"stale").unwrap();
        let service = McpService::new(dir.clone());
        let status = service.status();
        assert!(!status.enabled);
        assert!(!service.endpoint_path.exists());
        assert!(!service.secret_path.exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn bounded_line_rejects_oversized_unterminated_input() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let mut writer = TcpStream::connect(address).unwrap();
        let (server, _) = listener.accept().unwrap();
        writer.write_all(&vec![b'a'; MAX_AUTH_BYTES + 1]).unwrap();
        writer.flush().unwrap();

        let mut reader = BufReader::new(server);
        let mut line = Vec::new();
        let outcome = read_line_bounded(
            &mut reader,
            &mut line,
            MAX_AUTH_BYTES,
            None,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(matches!(outcome, BoundedLine::TooLong));
    }

    #[test]
    fn bounded_line_honors_the_absolute_deadline() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let _writer = TcpStream::connect(address).unwrap();
        let (server, _) = listener.accept().unwrap();
        server
            .set_read_timeout(Some(Duration::from_millis(20)))
            .unwrap();

        let started = Instant::now();
        let mut reader = BufReader::new(server);
        let mut line = Vec::new();
        let outcome = read_line_bounded(
            &mut reader,
            &mut line,
            MAX_AUTH_BYTES,
            Some(started + Duration::from_millis(80)),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(matches!(outcome, BoundedLine::TimedOut));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn stopping_waits_for_active_client_workers() {
        let dir = temp_dir("drain-clients");
        let service = McpService::new(dir.clone());
        let clients = Arc::new(ClientRegistry::default());
        clients.authenticated.store(1, Ordering::SeqCst);
        let (release, blocked) = std::sync::mpsc::channel();
        clients.add_worker(thread::spawn(move || {
            let _ = blocked.recv();
        }));
        let release_after_stop_starts = thread::spawn(move || {
            thread::sleep(Duration::from_millis(40));
            let _ = release.send(());
        });
        {
            let mut inner = service.inner.lock().unwrap();
            inner.running = Some(RunningServer {
                endpoint: "127.0.0.1:1".into(),
                stop: Arc::new(AtomicBool::new(false)),
                clients: Arc::clone(&clients),
                join: thread::spawn(|| {}),
            });
            let started = Instant::now();
            service.stop_locked(&mut inner, true);
            assert!(started.elapsed() >= Duration::from_millis(30));
        }
        release_after_stop_starts.join().unwrap();
        assert_eq!(clients.authenticated.load(Ordering::SeqCst), 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn worker_slot_limit_admits_exactly_the_documented_connection_count() {
        let open_workers = AtomicUsize::new(0);
        for _ in 0..MAX_CLIENT_WORKERS {
            assert!(reserve_worker_slot(&open_workers));
        }
        assert_eq!(open_workers.load(Ordering::SeqCst), MAX_CLIENT_WORKERS);
        assert!(!reserve_worker_slot(&open_workers));
        assert_eq!(open_workers.load(Ordering::SeqCst), MAX_CLIENT_WORKERS);

        open_workers.fetch_sub(1, Ordering::SeqCst);
        assert!(reserve_worker_slot(&open_workers));
        assert_eq!(open_workers.load(Ordering::SeqCst), MAX_CLIENT_WORKERS);
    }

    #[test]
    fn status_builds_a_ready_to_use_bridge_configuration() {
        let dir = temp_dir("status");
        let service = McpService::new(dir.clone());
        let status = service.status();
        assert!(status.supported);
        assert!(!status.enabled);
        assert!(status.client_config.contains("--mcp-stdio"));
        assert!(status.client_config.contains("--data-dir"));
        assert!(status
            .client_config
            .contains(&dir.to_string_lossy().to_string()));
        assert!(!status.client_config.contains(MCP_SECRET_FILE));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn finished_listener_is_cleared_and_runtime_files_are_removed() {
        let dir = temp_dir("finished-listener");
        let service = McpService::new(dir.clone());
        write_private_file(&service.secret_path, b"token").unwrap();
        std::fs::write(&service.endpoint_path, b"{}").unwrap();
        let join = std::thread::spawn(|| {});
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while !join.is_finished() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        {
            let mut inner = service.inner.lock().unwrap();
            inner.enabled = true;
            inner.running = Some(RunningServer {
                endpoint: "127.0.0.1:1".into(),
                stop: Arc::new(AtomicBool::new(false)),
                clients: Arc::new(ClientRegistry::default()),
                join,
            });
        }

        let status = service.status();
        assert!(status.enabled);
        assert!(!status.running);
        assert_eq!(
            status.error.as_deref(),
            Some("MCP listener stopped unexpectedly")
        );
        assert!(!service.endpoint_path.exists());
        assert!(!service.secret_path.exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn loopback_authentication_accepts_only_the_control_token() {
        let (accepted, ack) = authenticate_over_loopback("expected", "expected");
        assert!(accepted);
        assert_eq!(ack, r#"{"ok":true}"#.to_string());

        let (rejected, ack) = authenticate_over_loopback("expected", "wrong");
        assert!(!rejected);
        assert_eq!(ack, r#"{"ok":false}"#.to_string());
    }

    fn authenticate_over_loopback(expected: &str, supplied: &str) -> (bool, String) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let expected = expected.to_string();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            authorize_connection(&mut stream, &mut reader, &expected, &AtomicBool::new(false))
        });
        let mut client = TcpStream::connect(address).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let handshake = serde_json::to_vec(&serde_json::json!({ "token": supplied })).unwrap();
        client.write_all(&handshake).unwrap();
        client.write_all(b"\n").unwrap();
        client.flush().unwrap();
        let mut ack = String::new();
        client.read_to_string(&mut ack).unwrap();
        (server.join().unwrap(), ack.trim().to_string())
    }
}
