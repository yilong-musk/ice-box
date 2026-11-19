// SPDX-License-Identifier: GPL-3.0-or-later

//! Public exit IP of the current route, measured through the mixed inbound.
//!
//! The configured server address is often a hostname, and it is not the
//! address a destination sees. This probe sends one HTTPS request through the
//! local proxy to Cloudflare's trace endpoint and reads the IP and country
//! from the far end. Status reads only consult the cache; the request runs on
//! a background thread. See `docs/architecture.md` (Trust boundaries) and the
//! `probe_exit_ip` setting.

use std::io::Read;
use std::net::IpAddr;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const FRESH: Duration = Duration::from_secs(30);
const RETRY_AFTER_MISS: Duration = Duration::from_secs(15);
const TRACE_URL: &str = "https://www.cloudflare.com/cdn-cgi/trace";

/// One successful trace. `country` is absent when Cloudflare reports an
/// unknown location or a value that is not an ISO alpha-2 code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExitSample {
    pub ip: String,
    pub country: Option<String>,
}

struct Cache {
    generation: u64,
    key: String,
    fetched_at: Instant,
    sample: Option<ExitSample>,
    inflight_key: Option<String>,
}

impl Cache {
    fn empty() -> Self {
        Self {
            generation: 0,
            key: String::new(),
            fetched_at: Instant::now(),
            sample: None,
            inflight_key: None,
        }
    }
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Cache::empty()))
}

fn lock() -> std::sync::MutexGuard<'static, Cache> {
    cache().lock().unwrap_or_else(|err| err.into_inner())
}

/// Drop a sample from a core that is no longer running.
pub(crate) fn clear() {
    let mut slot = lock();
    let generation = slot.generation.wrapping_add(1);
    *slot = Cache::empty();
    slot.generation = generation;
}

/// Cached exit for `key`. Starts a probe when the sample is missing or stale.
/// Returns immediately; the first sample appears on a later status read.
pub(crate) fn observe(key: &str, proxy_url: &str) -> Option<ExitSample> {
    let mut slot = lock();
    let same = slot.key == key;
    let age = slot.fetched_at.elapsed();
    let fresh = same
        && if slot.sample.is_some() {
            age < FRESH
        } else {
            age < RETRY_AFTER_MISS
        };
    let current = if same { slot.sample.clone() } else { None };
    if fresh || slot.inflight_key.as_deref() == Some(key) {
        return current;
    }
    let generation = slot.generation;
    slot.inflight_key = Some(key.to_string());
    let probe_key = key.to_string();
    let proxy_url = proxy_url.to_string();
    drop(slot);
    let spawned = std::thread::Builder::new()
        .name("exit-probe".into())
        .spawn(move || {
            let sample = fetch_exit(&proxy_url);
            let mut slot = lock();
            if slot.generation != generation
                || slot.inflight_key.as_deref() != Some(probe_key.as_str())
            {
                return;
            }
            slot.inflight_key = None;
            slot.key = probe_key;
            slot.fetched_at = Instant::now();
            slot.sample = sample;
        });
    if spawned.is_err() {
        let mut slot = lock();
        if slot.inflight_key.as_deref() == Some(key) {
            slot.inflight_key = None;
        }
    }
    current
}

fn fetch_exit(proxy_url: &str) -> Option<ExitSample> {
    let proxy = ureq::Proxy::new(proxy_url).ok()?;
    let agent = ureq::AgentBuilder::new()
        .proxy(proxy)
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(5))
        .timeout_write(Duration::from_secs(5))
        .max_idle_connections(0)
        .build();
    read_limited(&agent, TRACE_URL).and_then(|body| parse_trace(&body))
}

fn read_limited(agent: &ureq::Agent, url: &str) -> Option<String> {
    let response = agent.get(url).call().ok()?;
    let mut body = String::new();
    response
        .into_reader()
        .take(4096)
        .read_to_string(&mut body)
        .ok()?;
    Some(body)
}

fn parse_trace(body: &str) -> Option<ExitSample> {
    let mut ip = None;
    let mut country = None;
    for line in body.lines() {
        if let Some(value) = line.strip_prefix("ip=") {
            let value = value.trim();
            if value.parse::<IpAddr>().is_ok() {
                ip = Some(value.to_string());
            }
        } else if let Some(value) = line.strip_prefix("loc=") {
            country = country_code(value);
        }
    }
    ip.map(|ip| ExitSample { ip, country })
}

/// Cloudflare's `loc` is an ISO 3166-1 alpha-2 country, or `XX` when unknown.
/// Tor (`T1`) and anything else that is not two letters is not a flag.
fn country_code(raw: &str) -> Option<String> {
    let value = raw.trim();
    let bytes = value.as_bytes();
    if bytes.len() != 2 || !bytes.iter().all(|byte| byte.is_ascii_alphabetic()) {
        return None;
    }
    let code = value.to_ascii_uppercase();
    if code == "XX" {
        return None;
    }
    Some(code)
}

#[cfg(test)]
mod tests {
    use super::{fetch_exit, parse_trace};

    #[test]
    fn trace_yields_the_observed_ip_and_country() {
        let body = "ip=1.36.1.1\nloc=hk\ncolo=HKG\n";
        let sample = parse_trace(body).expect("sample");
        assert_eq!(sample.ip, "1.36.1.1");
        assert_eq!(sample.country.as_deref(), Some("HK"));
    }

    #[test]
    fn trace_drops_a_country_that_has_no_flag() {
        let unknown = parse_trace("ip=1.36.1.1\nloc=XX\n").expect("ip");
        assert_eq!(unknown.country, None);
        let tor = parse_trace("ip=1.36.1.1\nloc=T1\n").expect("ip");
        assert_eq!(tor.country, None);
        let colo = parse_trace("ip=1.36.1.1\nloc=HKG\n").expect("ip");
        assert_eq!(colo.country, None);
        assert!(parse_trace("loc=HK\ncolo=HKG\n").is_none());
    }

    #[test]
    fn a_closed_proxy_does_not_invent_an_address() {
        assert!(fetch_exit("http://127.0.0.1:1").is_none());
    }
}
