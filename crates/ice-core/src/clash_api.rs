//! sing-box Clash API helpers (loopback only; sing-box 1.13.x compatible).

use std::io::{BufRead, BufReader, Read};
use std::sync::OnceLock;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::CoreError;
use crate::health::HealthEndpoints;
use ice_config::is_loopback_host;

/// Selector outbound tag in generated config (`ice-config` template).
pub const SELECTOR_TAG: &str = "proxy";

pub const DELAY_TEST_URL: &str = "http://www.gstatic.com/generate_204";

pub const CLASH_HTTP_TIMEOUT: Duration = Duration::from_secs(8);

/// Per-second delta from `GET /traffic` (`{"up": bytes, "down": bytes}`).
pub const TRAFFIC_SAMPLE_TIMEOUT: Duration = Duration::from_secs(3);

fn ensure_loopback(endpoints: &HealthEndpoints) -> Result<(), CoreError> {
    if !is_loopback_host(&endpoints.host) {
        return Err(CoreError::SpawnFailed(format!(
            "clash api host must be loopback, got {}",
            endpoints.host
        )));
    }
    Ok(())
}

fn base_url(endpoints: &HealthEndpoints) -> Result<String, CoreError> {
    ensure_loopback(endpoints)?;
    let host = endpoints.host.trim_matches(|c| c == '[' || c == ']');
    Ok(format!("http://{}:{}", host, endpoints.port))
}

/// One process-wide agent. Connection pooling is disabled on purpose: a
/// pooled idle connection to a core that died stays half-closed (CLOSE_WAIT)
/// on macOS, and its 4-tuple keeps the clash port occupied — a fresh
/// listener bind on the same port then fails with `EADDRINUSE`. The app
/// polls the clash API every second or two, so the extra loopback handshake
/// is negligible.
fn shared_agent() -> ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT
        .get_or_init(|| {
            ureq::AgentBuilder::new()
                .timeout_connect(CLASH_HTTP_TIMEOUT)
                .timeout_read(CLASH_HTTP_TIMEOUT)
                .timeout_write(CLASH_HTTP_TIMEOUT)
                .max_idle_connections(0)
                .build()
        })
        .clone()
}

fn clash_get(endpoints: &HealthEndpoints, path: &str) -> Result<String, CoreError> {
    let url = format!("{}{}", base_url(endpoints)?, path);
    let agent = shared_agent();
    let response = agent
        .get(&url)
        .call()
        .map_err(|e| CoreError::SpawnFailed(format!("clash api GET {path}: {e}")))?;
    let status = response.status();
    let mut body = String::new();
    response
        .into_reader()
        .take(256 * 1024)
        .read_to_string(&mut body)
        .map_err(|e| CoreError::SpawnFailed(format!("clash api read: {e}")))?;
    if !(200..300).contains(&status) {
        return Err(CoreError::SpawnFailed(format!(
            "clash api GET {path} HTTP {status}: {body}"
        )));
    }
    Ok(body)
}

fn clash_put_json(endpoints: &HealthEndpoints, path: &str, json: &str) -> Result<(), CoreError> {
    let url = format!("{}{}", base_url(endpoints)?, path);
    let agent = shared_agent();
    let response = agent
        .put(&url)
        .set("Content-Type", "application/json")
        .send_string(json)
        .map_err(|e| CoreError::SpawnFailed(format!("clash api PUT {path}: {e}")))?;
    let status = response.status();
    if !(200..300).contains(&status) {
        let mut text = String::new();
        let _ = response.into_reader().take(512).read_to_string(&mut text);
        return Err(CoreError::SpawnFailed(format!(
            "clash api PUT {path} HTTP {status}: {text}"
        )));
    }
    Ok(())
}

fn clash_patch_json(endpoints: &HealthEndpoints, path: &str, json: &str) -> Result<(), CoreError> {
    let url = format!("{}{}", base_url(endpoints)?, path);
    let agent = shared_agent();
    let response = agent
        .patch(&url)
        .set("Content-Type", "application/json")
        .send_string(json)
        .map_err(|e| CoreError::SpawnFailed(format!("clash api PATCH {path}: {e}")))?;
    let status = response.status();
    if !(200..300).contains(&status) {
        let mut text = String::new();
        let _ = response.into_reader().take(512).read_to_string(&mut text);
        return Err(CoreError::SpawnFailed(format!(
            "clash api PATCH {path} HTTP {status}: {text}"
        )));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct DelayResponse {
    delay: u32,
}

/// TCP/HTTP delay test for a leaf outbound (`GET /proxies/{tag}/delay`).
pub fn proxy_delay(
    endpoints: &HealthEndpoints,
    tag: &str,
    timeout_ms: u32,
    test_url: &str,
) -> Result<u32, CoreError> {
    let path = format!(
        "/proxies/{}/delay?timeout={}&url={}",
        percent_encode_path(tag),
        timeout_ms,
        percent_encode_query(test_url)
    );
    let body = clash_get(endpoints, &path)?;
    let parsed: DelayResponse = serde_json::from_str(&body)
        .map_err(|e| CoreError::SpawnFailed(format!("clash delay parse: {e}; body={body}")))?;
    Ok(parsed.delay)
}

/// Switch a selector group member without full config reload (`PUT /proxies/{group}`).
pub fn select_group(
    endpoints: &HealthEndpoints,
    group_tag: &str,
    member_tag: &str,
) -> Result<(), CoreError> {
    let path = format!("/proxies/{}", percent_encode_path(group_tag));
    let body = serde_json::json!({ "name": member_tag }).to_string();
    clash_put_json(endpoints, &path, &body)
}

/// Switch the top-level selector without full config reload (`PUT /proxies/proxy`).
pub fn select_outbound(endpoints: &HealthEndpoints, tag: &str) -> Result<(), CoreError> {
    select_group(endpoints, SELECTOR_TAG, tag)
}

#[derive(Debug, Deserialize)]
struct ConfigsResponse {
    mode: String,
}

/// Read the current Clash runtime mode (`GET /configs`). Used for status display and
/// verifying a `PATCH` took effect.
pub fn get_mode(endpoints: &HealthEndpoints) -> Result<String, CoreError> {
    let body = clash_get(endpoints, "/configs")?;
    let parsed: ConfigsResponse = serde_json::from_str(&body)
        .map_err(|e| CoreError::SpawnFailed(format!("clash configs parse: {e}; body={body}")))?;
    Ok(parsed.mode)
}

/// Switch the Clash runtime mode (`PATCH /configs` with `{"mode": ...}`). sing-box
/// validates the mode against its runtime `mode-list`, which under the pinned 1.13.19 is
/// always `[<default_mode>]` (an emitted `mode_list` is rejected; the built-in list is just
/// `default_mode` prepended onto an empty list). A `PATCH` targeting a different mode is
/// therefore silently ignored — `GET /configs` keeps returning the old mode — so callers
/// must verify with [`get_mode`] and fall back to a rebuild + reload. Pass a mode string
/// from `ice_config::clash_mode_name` so the reported mode stays capitalized.
pub fn set_mode(endpoints: &HealthEndpoints, mode: &str) -> Result<(), CoreError> {
    let body = serde_json::json!({ "mode": mode }).to_string();
    clash_patch_json(endpoints, "/configs", &body)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupState {
    pub tag: String,
    /// Clash API group type: `Selector` / `URLTest` / `Fallback` / `LoadBalance`.
    pub group_type: String,
    /// Currently selected (Selector) or active (URLTest / Fallback) member; empty when none.
    pub now: String,
    /// Member tags of the group.
    pub all: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ProxiesResponse {
    proxies: std::collections::HashMap<String, ProxyInfo>,
}

#[derive(Debug, Deserialize)]
struct ProxyInfo {
    #[serde(rename = "type")]
    r#type: String,
    #[serde(default)]
    now: String,
    #[serde(default)]
    all: Vec<String>,
}

/// Live strategy group state via `GET /proxies` (sing-box Clash API).
pub fn proxy_groups(endpoints: &HealthEndpoints) -> Result<Vec<GroupState>, CoreError> {
    let body = clash_get(endpoints, "/proxies")?;
    let parsed: ProxiesResponse = serde_json::from_str(&body)
        .map_err(|e| CoreError::SpawnFailed(format!("clash proxies parse: {e}; body={body}")))?;
    Ok(proxy_groups_filter(parsed.proxies))
}

fn proxy_groups_filter(proxies: std::collections::HashMap<String, ProxyInfo>) -> Vec<GroupState> {
    let mut groups: Vec<GroupState> = proxies
        .into_iter()
        .filter_map(|(tag, info)| {
            if info.all.is_empty() {
                return None;
            }
            Some(GroupState {
                tag,
                group_type: info.r#type,
                now: info.now,
                all: info.all,
            })
        })
        .collect();
    groups.sort_by(|a, b| a.tag.cmp(&b.tag));
    groups
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrafficSample {
    pub up: u64,
    pub down: u64,
}

#[derive(Debug, Deserialize)]
struct TrafficDelta {
    up: u64,
    down: u64,
}

/// Why a `/traffic` follow stopped without an I/O error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrafficStreamEnd {
    /// `on_sample` returned false (shutdown or retarget).
    Stopped,
    /// Upstream closed the body without a read error.
    Eof,
}

/// Read one per-second sample from chunked `GET /traffic` (closes after first tick).
pub fn traffic_sample(endpoints: &HealthEndpoints) -> Result<TrafficSample, CoreError> {
    let mut found = None;
    traffic_foreach(endpoints, TRAFFIC_SAMPLE_TIMEOUT, |sample| {
        found = Some(sample);
        false
    })?;
    found.ok_or_else(|| CoreError::SpawnFailed("clash traffic stream ended without sample".into()))
}

/// Follow Clash `GET /traffic` and invoke `on_sample` for each JSON tick.
///
/// Return `false` from `on_sample` to disconnect as [`TrafficStreamEnd::Stopped`].
/// A clean stream close is [`TrafficStreamEnd::Eof`]. Read timeouts and HTTP
/// failures are errors so the caller can back off.
pub(crate) fn traffic_foreach(
    endpoints: &HealthEndpoints,
    read_timeout: Duration,
    mut on_sample: impl FnMut(TrafficSample) -> bool,
) -> Result<TrafficStreamEnd, CoreError> {
    let url = format!("{}/traffic", base_url(endpoints)?);
    // A fresh agent per stream, with pooling disabled: when the stream ends
    // the connection must close immediately (a pooled half-closed connection
    // would keep the clash port occupied on macOS and block the next core's
    // listener bind).
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(CLASH_HTTP_TIMEOUT)
        .timeout_read(read_timeout)
        .max_idle_connections(0)
        .build();
    let response = agent
        .get(&url)
        .call()
        .map_err(|e| CoreError::SpawnFailed(format!("clash api GET /traffic: {e}")))?;
    let status = response.status();
    if !(200..300).contains(&status) {
        return Err(CoreError::SpawnFailed(format!(
            "clash api GET /traffic HTTP {status}"
        )));
    }
    let reader = BufReader::new(response.into_reader());
    for line in reader.lines() {
        let line = line.map_err(|e| CoreError::SpawnFailed(format!("clash traffic read: {e}")))?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(delta) = serde_json::from_str::<TrafficDelta>(trimmed) {
            if !on_sample(TrafficSample {
                up: delta.up,
                down: delta.down,
            }) {
                return Ok(TrafficStreamEnd::Stopped);
            }
        }
    }
    Ok(TrafficStreamEnd::Eof)
}

fn percent_encode_path(s: &str) -> String {
    urlencoding::encode(s).into_owned()
}

fn percent_encode_query(s: &str) -> String {
    urlencoding::encode(s).into_owned()
}

/// Test-only recorded HTTP request (shared mock Clash API, see [`MockClashApi`]).
#[doc(hidden)]
#[derive(Debug, Clone)]
pub struct RecordedRequest {
    pub method: String,
    pub path: String,
    pub body: String,
}

/// Test-only stateful mock of the sing-box Clash API, shared with the desktop crate's
/// orchestrate tests. It serves `GET /configs` with the current mode and applies a mode
/// change on 2xx `PATCH /configs`. Not part of the public API.
#[doc(hidden)]
pub struct MockClashApi {
    pub addr: std::net::SocketAddr,
    pub requests: std::sync::Arc<std::sync::Mutex<Vec<RecordedRequest>>>,
    mode: std::sync::Arc<std::sync::Mutex<String>>,
    thread: Option<std::thread::JoinHandle<()>>,
    stop: std::sync::mpsc::Sender<()>,
}

impl MockClashApi {
    /// Spawn a mock where a 2xx `PATCH /configs` records the request, applies the new mode,
    /// and `GET /configs` returns it; non-2xx `patch_status` makes every request fail with
    /// that status plus `body`.
    pub fn start(patch_status: u16, initial_mode: &str) -> Self {
        Self::start_with(patch_status, initial_mode, true)
    }

    /// Like [`MockClashApi::start`] but a 2xx `PATCH` does not change the served mode
    /// (simulates a silently-ignored `PATCH /configs`).
    pub fn start_with_ignored_patch(patch_status: u16, initial_mode: &str) -> Self {
        Self::start_with(patch_status, initial_mode, false)
    }

    fn start_with(patch_status: u16, initial_mode: &str, patch_applies: bool) -> Self {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let requests: std::sync::Arc<std::sync::Mutex<Vec<RecordedRequest>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mode: std::sync::Arc<std::sync::Mutex<String>> =
            std::sync::Arc::new(std::sync::Mutex::new(initial_mode.to_string()));
        let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
        let reqs = requests.clone();
        let mode_shared = mode.clone();
        let thread = std::thread::spawn(move || loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let reqs = reqs.clone();
                    let mode = mode_shared.clone();
                    let _ = std::thread::spawn(move || {
                        let mut buf = [0u8; 65536];
                        let mut read = 0usize;
                        loop {
                            match stream.read(&mut buf[read..]) {
                                Ok(0) => break,
                                Ok(n) => {
                                    read += n;
                                    if buf[..read].windows(4).any(|w| w == b"\r\n\r\n")
                                        || read >= buf.len()
                                    {
                                        break;
                                    }
                                }
                                Err(_) => break,
                            }
                        }
                        let head = String::from_utf8_lossy(&buf[..read]).to_string();
                        let request_line = head.lines().next().unwrap_or("").to_string();
                        let (method, path) = request_line
                            .split_once(' ')
                            .map(|(m, rest)| {
                                (
                                    m.to_string(),
                                    rest.split(' ').next().unwrap_or("").to_string(),
                                )
                            })
                            .unwrap_or_default();
                        let mut content_length = 0usize;
                        for line in head.lines().skip(1) {
                            if let Some((k, v)) = line.split_once(':') {
                                if k.trim().eq_ignore_ascii_case("content-length") {
                                    content_length = v.trim().parse().unwrap_or(0);
                                }
                            }
                        }
                        let header_end = buf[..read]
                            .windows(4)
                            .position(|w| w == b"\r\n\r\n")
                            .map(|p| p + 4)
                            .unwrap_or(read);
                        let mut body_data =
                            String::from_utf8_lossy(&buf[header_end..read]).to_string();
                        while body_data.len() < content_length {
                            let mut more = [0u8; 4096];
                            match stream.read(&mut more) {
                                Ok(0) => break,
                                Ok(n) => body_data.push_str(&String::from_utf8_lossy(&more[..n])),
                                Err(_) => break,
                            }
                        }
                        let is_configs = path == "/configs";
                        let is_2xx = (200..300).contains(&patch_status);
                        if method == "PATCH" && is_configs && is_2xx && patch_applies {
                            if let Ok(parsed) =
                                serde_json::from_str::<serde_json::Value>(&body_data)
                            {
                                if let Some(next) = parsed.get("mode").and_then(|m| m.as_str()) {
                                    *mode.lock().unwrap() = next.to_string();
                                }
                            }
                        }
                        let resp = match (method.as_str(), path.as_str()) {
                            ("GET", "/traffic") if is_2xx => {
                                reqs.lock().unwrap().push(RecordedRequest {
                                    method,
                                    path,
                                    body: body_data,
                                });
                                let header = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\n\r\n";
                                if stream.write_all(header.as_bytes()).is_err() {
                                    return;
                                }
                                for i in 0..40u64 {
                                    let line = format!(
                                        "{{\"up\":{},\"down\":{}}}\n",
                                        100 + i,
                                        200 + i
                                    );
                                    if stream.write_all(line.as_bytes()).is_err() {
                                        break;
                                    }
                                    let _ = stream.flush();
                                    std::thread::sleep(Duration::from_millis(25));
                                }
                                return;
                            }
                            ("GET", "/configs") if is_2xx => {
                                let current = mode.lock().unwrap().clone();
                                let body = serde_json::json!({
                                    "mode": current,
                                    "mode-list": ["Rule", "Global", "Direct"],
                                })
                                .to_string();
                                format!(
                                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                    body.len(),
                                    body
                                )
                            }
                            ("PATCH", "/configs") if is_2xx => {
                                "HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                                    .to_string()
                            }
                            _ => {
                                let status = if patch_status == 204 { 200 } else { patch_status };
                                format!(
                                    "HTTP/1.1 {status} Mock\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                    body_data.len(),
                                    body_data
                                )
                            }
                        };
                        reqs.lock().unwrap().push(RecordedRequest {
                            method,
                            path,
                            body: body_data,
                        });
                        let _ = stream.write_all(resp.as_bytes());
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if stop_rx.try_recv().is_ok() {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break,
            }
        });
        Self {
            addr,
            requests,
            mode,
            thread: Some(thread),
            stop: stop_tx,
        }
    }

    pub fn endpoints(&self) -> HealthEndpoints {
        HealthEndpoints {
            host: "127.0.0.1".into(),
            port: self.addr.port(),
        }
    }

    /// Mode currently served by `GET /configs`.
    pub fn current_mode(&self) -> String {
        self.mode.lock().unwrap().clone()
    }
}

impl Drop for MockClashApi {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::HealthEndpoints;
    use std::time::Duration;

    #[test]
    fn delay_response_deserializes() {
        let v: DelayResponse = serde_json::from_str(r#"{"delay": 42}"#).unwrap();
        assert_eq!(v.delay, 42);
    }

    #[test]
    fn traffic_delta_deserializes() {
        let v: TrafficDelta = serde_json::from_str(r#"{"up":1024,"down":4096}"#).unwrap();
        assert_eq!(v.up, 1024);
        assert_eq!(v.down, 4096);
    }

    #[test]
    fn traffic_sample_reads_first_tick() {
        let server = MockClashApi::start(200, "Rule");
        let sample = traffic_sample(&server.endpoints()).expect("first tick");
        assert_eq!(sample.up, 100);
        assert_eq!(sample.down, 200);
    }

    #[test]
    fn traffic_foreach_reports_stopped_when_callback_returns_false() {
        let server = MockClashApi::start(200, "Rule");
        let end = traffic_foreach(&server.endpoints(), TRAFFIC_SAMPLE_TIMEOUT, |_| false)
            .expect("stopped");
        assert_eq!(end, TrafficStreamEnd::Stopped);
    }

    #[test]
    fn traffic_foreach_reports_eof_when_stream_ends() {
        let server = MockClashApi::start(200, "Rule");
        let mut ticks = 0usize;
        let end = traffic_foreach(&server.endpoints(), TRAFFIC_SAMPLE_TIMEOUT, |_| {
            ticks += 1;
            true
        })
        .expect("eof");
        assert!(ticks > 0, "expected at least one tick before close");
        assert_eq!(end, TrafficStreamEnd::Eof);
    }

    #[test]
    fn proxies_response_parses_groups_only() {
        let v: ProxiesResponse = serde_json::from_str(
            r#"{
                "proxies": {
                    "proxy": {"type":"Selector","now":"node-a","all":["node-a","node-b","direct"]},
                    "auto": {"type":"URLTest","now":"node-b","all":["node-a","node-b"]},
                    "direct": {"type":"Direct","now":"direct","all":[]}
                }
            }"#,
        )
        .unwrap();
        let groups = super::proxy_groups_filter(v.proxies);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].tag, "auto");
        assert_eq!(groups[0].now, "node-b");
        assert_eq!(groups[0].group_type, "URLTest");
        assert_eq!(groups[1].tag, "proxy");
        assert_eq!(groups[1].now, "node-a");
        assert_eq!(groups[1].all, vec!["node-a", "node-b", "direct"]);
    }

    #[test]
    fn non_loopback_endpoints_rejected() {
        let endpoints = HealthEndpoints {
            host: "0.0.0.0".into(),
            port: 19090,
        };
        let err = proxy_delay(&endpoints, "n1", 1000, DELAY_TEST_URL).expect_err("reject");
        assert!(err.to_string().contains("loopback"));
    }

    // --- Slice 4c: runtime mode switch against the shared mock Clash API server ---

    #[test]
    fn set_mode_patches_configs_with_capitalized_mode() {
        let server = MockClashApi::start(204, "Rule");
        set_mode(&server.endpoints(), "Global").expect("set mode");
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(
            server.current_mode(),
            "Global",
            "mock must apply the PATCHed mode"
        );
        let reqs = server.requests.lock().unwrap();
        assert_eq!(reqs.len(), 1, "expected exactly one PATCH");
        assert_eq!(reqs[0].method, "PATCH");
        assert_eq!(reqs[0].path, "/configs");
        let body: serde_json::Value = serde_json::from_str(&reqs[0].body).unwrap();
        assert_eq!(body["mode"], "Global");
    }

    #[test]
    fn set_mode_non_2xx_is_error() {
        let server = MockClashApi::start(400, "Rule");
        let err = set_mode(&server.endpoints(), "Global").expect_err("400");
        assert!(err.to_string().contains("400"), "err: {err}");
    }

    #[test]
    fn get_mode_parses_configs_response() {
        let server = MockClashApi::start(200, "Global");
        assert_eq!(get_mode(&server.endpoints()).unwrap(), "Global");
    }

    #[test]
    fn non_loopback_endpoints_rejected_for_mode() {
        let endpoints = HealthEndpoints {
            host: "0.0.0.0".into(),
            port: 19090,
        };
        let err = set_mode(&endpoints, "Global").expect_err("reject");
        assert!(err.to_string().contains("loopback"));
    }
}
