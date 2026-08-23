//! sing-box Clash API helpers (loopback only; sing-box 1.13.x compatible).

use std::io::{BufRead, BufReader, Read};
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

fn clash_get(endpoints: &HealthEndpoints, path: &str) -> Result<String, CoreError> {
    let url = format!("{}{}", base_url(endpoints)?, path);
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(CLASH_HTTP_TIMEOUT)
        .timeout_read(CLASH_HTTP_TIMEOUT)
        .build();
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
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(CLASH_HTTP_TIMEOUT)
        .timeout_read(CLASH_HTTP_TIMEOUT)
        .timeout_write(CLASH_HTTP_TIMEOUT)
        .build();
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

#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct ConnectionStats {
    pub connection_count: usize,
}

#[derive(Debug, Deserialize)]
struct ConnectionsResponse {
    #[serde(default)]
    connections: Vec<serde_json::Value>,
}

/// Active connection count via `GET /connections`.
pub fn connection_stats(endpoints: &HealthEndpoints) -> Result<ConnectionStats, CoreError> {
    let body = clash_get(endpoints, "/connections")?;
    let parsed: ConnectionsResponse = serde_json::from_str(&body)
        .map_err(|e| CoreError::SpawnFailed(format!("clash connections parse: {e}")))?;
    Ok(ConnectionStats {
        connection_count: parsed.connections.len(),
    })
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

/// Read one per-second sample from chunked `GET /traffic` (closes after first tick).
pub fn traffic_sample(endpoints: &HealthEndpoints) -> Result<TrafficSample, CoreError> {
    let url = format!("{}/traffic", base_url(endpoints)?);
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(CLASH_HTTP_TIMEOUT)
        .timeout_read(TRAFFIC_SAMPLE_TIMEOUT)
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
            return Ok(TrafficSample {
                up: delta.up,
                down: delta.down,
            });
        }
    }
    Err(CoreError::SpawnFailed(
        "clash traffic stream ended without sample".into(),
    ))
}

fn percent_encode_path(s: &str) -> String {
    urlencoding::encode(s).into_owned()
}

fn percent_encode_query(s: &str) -> String {
    urlencoding::encode(s).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::HealthEndpoints;

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
}
