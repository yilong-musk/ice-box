// SPDX-License-Identifier: GPL-3.0-or-later

//! Clash API / mixed inbound health probe.

use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::error::CoreError;
use ice_types::is_loopback_host;

/// Default healthcheck timeout: **5000 ms**.
pub const HEALTHCHECK_TIMEOUT: Duration = Duration::from_millis(5000);

/// Poll interval while waiting for the port to accept connections.
pub const HEALTHCHECK_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Per-attempt Clash `GET /version` timeout. Kept short so
/// [`TcpHealthProbe::wait_healthy_until`] can retry inside
/// [`HEALTHCHECK_TIMEOUT`] while sing-box tears down and rebuilds
/// listeners after SIGHUP.
pub const HEALTH_HTTP_TIMEOUT: Duration = Duration::from_millis(500);

/// Endpoints used after spawn. v1 probes **TCP connect** then **HTTP GET /version**
/// on the Clash API listen address so a stray process holding the port is not
/// treated as a healthy core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthEndpoints {
    pub host: String,
    pub port: u16,
}

impl HealthEndpoints {
    pub fn socket_addr_hint(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

pub trait HealthProbe: Send + Clone + 'static {
    fn wait_ready(&self, endpoints: &HealthEndpoints, timeout: Duration) -> Result<(), CoreError>;

    /// One-shot HTTP probe (`GET /version`). Default succeeds so test fakes that
    /// only model TCP still compile; production [`TcpHealthProbe`] overrides.
    fn probe_http(&self, endpoints: &HealthEndpoints) -> Result<(), CoreError> {
        let _ = endpoints;
        Ok(())
    }

    /// TCP connect plus HTTP until both succeed, or `timeout`.
    ///
    /// Test fakes stay single-shot (`wait_ready` then one `probe_http`) so
    /// sequenced probes are not consumed twice. [`TcpHealthProbe`] retries
    /// both steps: after SIGHUP the Clash listener is torn down and rebuilt,
    /// so a TCP connect on the dying socket followed by one `GET /version`
    /// is not enough.
    fn wait_healthy(
        &self,
        endpoints: &HealthEndpoints,
        timeout: Duration,
    ) -> Result<(), CoreError> {
        self.wait_healthy_until(endpoints, timeout, None)
    }

    fn wait_healthy_until(
        &self,
        endpoints: &HealthEndpoints,
        timeout: Duration,
        cancel: Option<&AtomicBool>,
    ) -> Result<(), CoreError> {
        if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
            return Err(CoreError::HealthcheckFailed(
                "cancelled while waiting for clash API".into(),
            ));
        }
        self.wait_ready(endpoints, timeout)?;
        if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
            return Err(CoreError::HealthcheckFailed(
                "cancelled while waiting for clash API".into(),
            ));
        }
        self.probe_http(endpoints)
    }
}

/// TCP connect probe against clash API (or any listen port).
#[derive(Debug, Default, Clone, Copy)]
pub struct TcpHealthProbe;

impl HealthProbe for TcpHealthProbe {
    fn wait_ready(&self, endpoints: &HealthEndpoints, timeout: Duration) -> Result<(), CoreError> {
        wait_tcp_ready(endpoints, timeout)
    }

    fn probe_http(&self, endpoints: &HealthEndpoints) -> Result<(), CoreError> {
        crate::clash_api::probe_version(endpoints)
            .map_err(|err| CoreError::HealthcheckFailed(format!("clash api GET /version: {err}")))
    }

    fn wait_healthy_until(
        &self,
        endpoints: &HealthEndpoints,
        timeout: Duration,
        cancel: Option<&AtomicBool>,
    ) -> Result<(), CoreError> {
        wait_tcp_and_http_ready(self, endpoints, timeout, cancel)
    }
}

fn wait_tcp_and_http_ready<H: HealthProbe>(
    probe: &H,
    endpoints: &HealthEndpoints,
    timeout: Duration,
    cancel: Option<&AtomicBool>,
) -> Result<(), CoreError> {
    let deadline = Instant::now() + timeout;
    let mut last_err = String::from("not attempted");

    while Instant::now() < deadline {
        if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
            return Err(CoreError::HealthcheckFailed(
                "cancelled while waiting for clash API".into(),
            ));
        }
        match tcp_connect_once(endpoints) {
            Ok(()) => match probe.probe_http(endpoints) {
                Ok(()) => return Ok(()),
                Err(e) => last_err = e.to_string(),
            },
            Err(e) => last_err = e,
        }
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        std::thread::sleep(HEALTHCHECK_POLL_INTERVAL.min(deadline.saturating_duration_since(now)));
    }

    Err(CoreError::HealthcheckFailed(format!(
        "timeout after {}ms waiting for clash api {}:{}: {last_err}",
        timeout.as_millis(),
        endpoints.host,
        endpoints.port
    )))
}

/// One TCP connect attempt against the Clash listen address.
pub(crate) fn tcp_connect_once(endpoints: &HealthEndpoints) -> Result<(), String> {
    if !is_loopback_host(&endpoints.host) {
        return Err(format!(
            "healthcheck host must be loopback, got {}",
            endpoints.host
        ));
    }
    let addr_str = endpoints.socket_addr_hint();
    let addrs: Vec<SocketAddr> = addr_str
        .to_socket_addrs()
        .map_err(|e| format!("resolve {addr_str}: {e}"))?
        .collect();
    if addrs.is_empty() {
        return Err(format!("no addresses for {addr_str}"));
    }
    let mut last = String::from("not attempted");
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, Duration::from_millis(200)) {
            Ok(_) => return Ok(()),
            Err(e) => last = e.to_string(),
        }
    }
    Err(last)
}

pub fn wait_tcp_ready(endpoints: &HealthEndpoints, timeout: Duration) -> Result<(), CoreError> {
    wait_tcp_ready_until(endpoints, timeout, None)
}

/// Like [`wait_tcp_ready`], but aborts early when `cancel` is set (app quit during auto-start).
pub fn wait_tcp_ready_until(
    endpoints: &HealthEndpoints,
    timeout: Duration,
    cancel: Option<&AtomicBool>,
) -> Result<(), CoreError> {
    if !is_loopback_host(&endpoints.host) {
        return Err(CoreError::HealthcheckFailed(format!(
            "healthcheck host must be loopback, got {}",
            endpoints.host
        )));
    }
    let addr_str = endpoints.socket_addr_hint();
    let addrs: Vec<SocketAddr> = addr_str
        .to_socket_addrs()
        .map_err(|e| CoreError::HealthcheckFailed(format!("resolve {addr_str}: {e}")))?
        .collect();

    if addrs.is_empty() {
        return Err(CoreError::HealthcheckFailed(format!(
            "no addresses for {addr_str}"
        )));
    }

    let deadline = Instant::now() + timeout;
    let mut last_err = String::from("not attempted");

    while Instant::now() < deadline {
        if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
            return Err(CoreError::HealthcheckFailed(
                "cancelled while waiting for clash API".into(),
            ));
        }
        for addr in &addrs {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            match TcpStream::connect_timeout(addr, remaining.min(Duration::from_millis(200))) {
                Ok(_) => return Ok(()),
                Err(e) => last_err = e.to_string(),
            }
        }
        std::thread::sleep(HEALTHCHECK_POLL_INTERVAL);
    }

    Err(CoreError::HealthcheckFailed(format!(
        "timeout after {}ms waiting for {addr_str}: {last_err}",
        timeout.as_millis()
    )))
}

/// Shared cancel flag for interrupting an in-flight healthcheck (quit during auto-start).
pub type HealthCancel = Arc<AtomicBool>;

/// Whether something is already accepting TCP on `host:port` (port conflict).
pub fn tcp_port_is_in_use(host: &str, port: u16) -> bool {
    let addr = format!("{host}:{port}");
    let Ok(addrs) = addr.to_socket_addrs() else {
        return false;
    };
    for a in addrs {
        if TcpStream::connect_timeout(&a, Duration::from_millis(150)).is_ok() {
            return true;
        }
    }
    false
}

/// Try to bind `host:port` briefly to confirm we could own the listen address.
/// Used as a stronger check for `0.0.0.0` / dual-stack conflicts.
pub fn tcp_bind_available(host: &str, port: u16) -> bool {
    let addr = format!("{host}:{port}");
    let Ok(addrs) = addr.to_socket_addrs() else {
        return true;
    };
    for a in addrs {
        match bind_like_core_listener(a) {
            Ok(listener) => {
                drop(listener);
                return true;
            }
            Err(_) => continue,
        }
    }
    false
}

/// Bind probe that mirrors the core's own listener options. sing-box (Go)
/// sets `SO_REUSEADDR` on its listeners; std's plain `TcpListener::bind`
/// does not. On macOS a loopback port whose previous connection is still in
/// `TIME_WAIT` (an old core's graceful close) is therefore bindable by the
/// core but reported as blocked by a plain probe — which would make the
/// port-release wait spin for the whole `TIME_WAIT` window even though the
/// real bind would succeed. With the option set, the probe only reports the
/// port blocked while a live holder exists (a live listener is not
/// overridable by `SO_REUSEADDR` alone), matching the core's own bind.
#[cfg(unix)]
fn bind_like_core_listener(addr: SocketAddr) -> std::io::Result<TcpListener> {
    use socket2::{Domain, Protocol, Socket, Type};

    let socket = Socket::new(Domain::for_address(addr), Type::STREAM, Some(Protocol::TCP))?;
    socket.set_reuse_address(true)?;
    socket.bind(&addr.into())?;
    Ok(socket.into())
}

#[cfg(not(unix))]
fn bind_like_core_listener(addr: SocketAddr) -> std::io::Result<TcpListener> {
    TcpListener::bind(addr)
}

/// Probe that never succeeds (for unit tests).
#[derive(Debug, Default, Clone, Copy)]
pub struct FailingHealthProbe;

impl HealthProbe for FailingHealthProbe {
    fn wait_ready(&self, endpoints: &HealthEndpoints, timeout: Duration) -> Result<(), CoreError> {
        let _ = (endpoints, timeout);
        Err(CoreError::HealthcheckFailed(
            "mock healthcheck failure".into(),
        ))
    }

    fn probe_http(&self, _endpoints: &HealthEndpoints) -> Result<(), CoreError> {
        Err(CoreError::HealthcheckFailed(
            "mock healthcheck failure".into(),
        ))
    }
}

/// Probe that always succeeds immediately.
#[derive(Debug, Default, Clone, Copy)]
pub struct ImmediateHealthProbe;

impl HealthProbe for ImmediateHealthProbe {
    fn wait_ready(
        &self,
        _endpoints: &HealthEndpoints,
        _timeout: Duration,
    ) -> Result<(), CoreError> {
        Ok(())
    }
}

/// Pops queued TCP results in order (for restart-fallback tests).
/// HTTP results default to `Ok` when the queue is empty so existing tests
/// keep a single pop per `wait_ready`.
#[derive(Debug, Clone)]
pub struct SequenceHealthProbe {
    results: std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<Result<(), CoreError>>>>,
    http: std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<Result<(), CoreError>>>>,
}

impl SequenceHealthProbe {
    pub fn new(results: Vec<Result<(), CoreError>>) -> Self {
        Self {
            results: std::sync::Arc::new(std::sync::Mutex::new(results.into())),
            http: std::sync::Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new())),
        }
    }

    pub fn with_http(self, http: Vec<Result<(), CoreError>>) -> Self {
        *self.http.lock().expect("lock") = http.into();
        self
    }
}

impl HealthProbe for SequenceHealthProbe {
    fn wait_ready(
        &self,
        _endpoints: &HealthEndpoints,
        _timeout: Duration,
    ) -> Result<(), CoreError> {
        let mut q = self.results.lock().expect("lock");
        match q.pop_front() {
            Some(r) => r,
            None => Ok(()),
        }
    }

    fn probe_http(&self, _endpoints: &HealthEndpoints) -> Result<(), CoreError> {
        let mut q = self.http.lock().expect("lock");
        match q.pop_front() {
            Some(r) => r,
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn wait_tcp_ready_until_aborts_when_cancel_set() {
        let endpoints = HealthEndpoints {
            host: "127.0.0.1".into(),
            // Unlikely to be listening; cancel should win before full timeout.
            port: 1,
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_bg = cancel.clone();
        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            cancel_bg.store(true, Ordering::SeqCst);
        });
        let t0 = Instant::now();
        let err = wait_tcp_ready_until(&endpoints, Duration::from_secs(5), Some(cancel.as_ref()))
            .expect_err("cancelled");
        assert!(err.to_string().contains("cancelled"));
        assert!(t0.elapsed() < Duration::from_secs(2));
        handle.join().unwrap();
    }

    #[test]
    fn tcp_port_is_in_use_detects_bound_listener() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(tcp_port_is_in_use("127.0.0.1", port));
        drop(listener);
        // Another process may legitimately reuse the ephemeral port between
        // drop() and this assertion. Port zero is never a connectable service,
        // so use it for the negative branch without relying on host state.
        assert!(!tcp_port_is_in_use("127.0.0.1", 0));
    }

    #[test]
    fn wait_healthy_retries_http_after_transient_failure() {
        use std::io::{Read, Write};
        use std::sync::atomic::AtomicU32;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();
        let remaining_http_fails = Arc::new(AtomicU32::new(2));
        let stop = Arc::new(AtomicBool::new(false));
        let fails = remaining_http_fails.clone();
        let stop_bg = stop.clone();
        let server = thread::spawn(move || {
            while !stop_bg.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream
                            .set_read_timeout(Some(Duration::from_millis(50)))
                            .ok();
                        let mut buf = [0u8; 2048];
                        let n = stream.read(&mut buf).unwrap_or(0);
                        if n == 0 {
                            continue;
                        }
                        if fails.fetch_sub(1, Ordering::SeqCst) > 0 {
                            continue;
                        }
                        let body = r#"{"version":"1.13.19"}"#;
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = stream.write_all(resp.as_bytes());
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });

        let endpoints = HealthEndpoints {
            host: "127.0.0.1".into(),
            port,
        };
        TcpHealthProbe
            .wait_healthy(&endpoints, Duration::from_secs(3))
            .expect("retries past two failed GET /version");
        stop.store(true, Ordering::SeqCst);
        let _ = server.join();
    }

    #[test]
    fn wait_healthy_times_out_when_http_never_arrives() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let endpoints = HealthEndpoints {
            host: "127.0.0.1".into(),
            port,
        };
        let err = TcpHealthProbe
            .wait_healthy(&endpoints, Duration::from_millis(400))
            .expect_err("http never served");
        assert!(
            err.to_string().contains("timeout") || err.to_string().contains("GET /version"),
            "{err}"
        );
        drop(listener);
    }
}
