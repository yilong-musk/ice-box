//! Clash API / mixed inbound health probe.

use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::error::CoreError;
use ice_config::is_loopback_host;

/// Default healthcheck timeout (architecture: 3–5s). Locked for v1: **5000 ms**.
pub const HEALTHCHECK_TIMEOUT: Duration = Duration::from_millis(5000);

/// Poll interval while waiting for the port to accept connections.
pub const HEALTHCHECK_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Endpoints used after spawn. v1 probes **TCP connect** to clash API listen address
/// (not HTTP yet; sufficient to know sing-box bound the controller port).
#[derive(Debug, Clone)]
pub struct HealthEndpoints {
    pub host: String,
    pub port: u16,
}

impl HealthEndpoints {
    pub fn socket_addr_hint(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

pub trait HealthProbe: Send {
    fn wait_ready(&self, endpoints: &HealthEndpoints, timeout: Duration) -> Result<(), CoreError>;
}

/// TCP connect probe against clash API (or any listen port).
#[derive(Debug, Default, Clone, Copy)]
pub struct TcpHealthProbe;

impl HealthProbe for TcpHealthProbe {
    fn wait_ready(&self, endpoints: &HealthEndpoints, timeout: Duration) -> Result<(), CoreError> {
        wait_tcp_ready(endpoints, timeout)
    }
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

/// Pops queued results in order (for restart-fallback tests).
#[derive(Debug, Clone)]
pub struct SequenceHealthProbe {
    results: std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<Result<(), CoreError>>>>,
}

impl SequenceHealthProbe {
    pub fn new(results: Vec<Result<(), CoreError>>) -> Self {
        Self {
            results: std::sync::Arc::new(std::sync::Mutex::new(results.into())),
        }
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
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
