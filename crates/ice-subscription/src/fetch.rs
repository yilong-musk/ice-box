// SPDX-License-Identifier: GPL-3.0-or-later

//! HTTP fetch: pinned direct or proxied, 60s total timeout, 20s per I/O
//! operation, 8 MiB cap.

use std::io::Read;
use std::time::{Duration, Instant};

use crate::error::SubscriptionError;
use crate::tls_fetch::{tls_client_config, tls_get_pinned, url_path_query};
use crate::url::{
    addrs_are_fake_ip, pin_url_to_ip, resolve_allowed_fetch_addrs, validate_subscription_url,
    validate_subscription_url_syntax,
};
use crate::userinfo::{parse_userinfo_header, SubscriptionUserInfo};

/// Hard body size limit (8 MiB).
pub const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;

/// Reject header values that would break HTTP framing (CRLF injection).
pub(crate) fn sanitize_http_header_value(value: &str) -> Result<String, SubscriptionError> {
    if value.chars().any(|c| matches!(c, '\r' | '\n' | '\0')) {
        return Err(SubscriptionError::FetchFailed(
            "conditional header value contains invalid characters".into(),
        ));
    }
    Ok(value.to_string())
}

/// Per-operation fetch timeout (connect, read, write).
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(20);

/// Hard wall-clock limit for one subscription fetch across every redirect hop
/// and the response body. Without it a provider that drips the body slowly
/// keeps resetting [`FETCH_TIMEOUT`] and the update never ends. DNS resolution
/// is left to the OS resolver; the hostname is normally cached and fast.
pub const FETCH_TOTAL_TIMEOUT: Duration = Duration::from_secs(60);

/// Max redirect hops; each target is SSRF-validated before follow.
const MAX_REDIRECTS: u32 = 5;

/// Error returned once the overall fetch deadline has passed.
fn fetch_timeout_error(log_url: &str) -> SubscriptionError {
    SubscriptionError::FetchFailed(format!(
        "GET {log_url}: timed out after {}s",
        FETCH_TOTAL_TIMEOUT.as_secs()
    ))
}

/// Time left before `deadline`, or a timeout error when it has passed.
pub(crate) fn fetch_time_left(
    log_url: &str,
    deadline: Instant,
) -> Result<Duration, SubscriptionError> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|left| !left.is_zero())
        .ok_or_else(|| fetch_timeout_error(log_url))
}

#[derive(Debug, Clone)]
pub struct FetchResponse {
    pub body: String,
    /// HTTP 304 Not Modified — body is empty; caller should keep cached subscription bytes.
    pub not_modified: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    /// Provider traffic counters (`subscription-userinfo`), when the response
    /// carried them — 304 responses may refresh usage without a new body.
    pub userinfo: Option<SubscriptionUserInfo>,
    pub content_disposition: Option<String>,
}

pub trait HttpFetcher: Send {
    /// True when this fetcher ignores OS / env proxies and connects directly
    /// (G5.12). [`DefaultFetcher`] reports false while it routes through a
    /// configured proxy.
    fn bypasses_system_proxy(&self) -> bool;

    fn get(
        &self,
        url: &str,
        etag: Option<&str>,
        last_modified: Option<&str>,
    ) -> Result<FetchResponse, SubscriptionError>;
}

/// One process-wide direct agent (no system proxy, no auto-redirects; the
/// redirect loop re-validates each hop). `Agent` is an `Arc` handle, so clones
/// are cheap and thread-safe — batch fetches share the pool across their threads.
fn build_direct_agent() -> ureq::Agent {
    static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    AGENT
        .get_or_init(|| {
            ureq::AgentBuilder::new()
                .timeout_connect(FETCH_TIMEOUT)
                .timeout_read(FETCH_TIMEOUT)
                .timeout_write(FETCH_TIMEOUT)
                .redirects(0)
                .build()
        })
        .clone()
}

/// One agent per configured proxy endpoint, so the fetch follows the same
/// proxy as browser traffic. TLS still uses the platform trust store.
fn build_proxy_agent(proxy_url: &str) -> Result<ureq::Agent, SubscriptionError> {
    let proxy = ureq::Proxy::new(proxy_url).map_err(|e| {
        SubscriptionError::FetchFailed(format!("invalid subscription proxy endpoint: {e}"))
    })?;
    Ok(ureq::AgentBuilder::new()
        .timeout_connect(FETCH_TIMEOUT)
        .timeout_read(FETCH_TIMEOUT)
        .timeout_write(FETCH_TIMEOUT)
        .redirects(0)
        .proxy(proxy)
        .tls_config(tls_client_config()?)
        .build())
}

/// Proxy endpoints a subscription fetch may use.
///
/// A request normally uses the endpoint matching its scheme, then falls back
/// to the other scheme and finally SOCKS. The fallback keeps a secure-web-only
/// setting useful for plain HTTP and an HTTP proxy useful for HTTPS via CONNECT.
#[derive(Debug, Clone, Default)]
pub struct SubscriptionProxy {
    pub http: Option<String>,
    pub https: Option<String>,
    pub socks: Option<String>,
}

impl SubscriptionProxy {
    fn normalized(mut self) -> Self {
        self.http = normalize_proxy_endpoint(self.http);
        self.https = normalize_proxy_endpoint(self.https);
        self.socks = normalize_proxy_endpoint(self.socks);
        self
    }

    fn is_empty(&self) -> bool {
        self.http.is_none() && self.https.is_none() && self.socks.is_none()
    }

    /// Endpoint selected for a URL, preferring the matching scheme.
    #[cfg(test)]
    fn for_url(&self, url: &str) -> Option<&str> {
        let (primary, secondary) = if url_is_https(url) {
            (&self.https, &self.http)
        } else {
            (&self.http, &self.https)
        };
        primary
            .as_deref()
            .or(secondary.as_deref())
            .or(self.socks.as_deref())
    }
}

fn normalize_proxy_endpoint(value: Option<String>) -> Option<String> {
    value
        .map(|url| url.trim().to_string())
        .filter(|url| !url.is_empty())
}

/// Agents built from [`SubscriptionProxy`], plus the direct fallback.
struct FetchAgents {
    http: Option<ureq::Agent>,
    https: Option<ureq::Agent>,
    socks: Option<ureq::Agent>,
    direct: ureq::Agent,
}

impl FetchAgents {
    fn direct() -> Self {
        Self {
            http: None,
            https: None,
            socks: None,
            direct: build_direct_agent(),
        }
    }

    fn from_proxy(proxy: &SubscriptionProxy) -> Result<Self, SubscriptionError> {
        Ok(Self {
            http: proxy.http.as_deref().map(build_proxy_agent).transpose()?,
            https: proxy.https.as_deref().map(build_proxy_agent).transpose()?,
            socks: proxy.socks.as_deref().map(build_proxy_agent).transpose()?,
            direct: build_direct_agent(),
        })
    }

    fn any(&self) -> bool {
        self.http.is_some() || self.https.is_some() || self.socks.is_some()
    }

    /// Agent for a URL, preferring the matching scheme.
    fn for_url(&self, url: &str) -> (&ureq::Agent, bool) {
        let (primary, secondary) = if url_is_https(url) {
            (&self.https, &self.http)
        } else {
            (&self.http, &self.https)
        };
        let proxy = primary
            .as_ref()
            .or(secondary.as_ref())
            .or(self.socks.as_ref());
        match proxy {
            Some(agent) => (agent, true),
            None => (&self.direct, false),
        }
    }
}

/// Subscription fetcher that follows the host's default network path.
///
/// With proxy endpoints it forwards requests through the matching proxy (the
/// app passes the OS system proxy, so behavior matches a browser). Without any
/// endpoint it keeps the pinned direct connect used by [`DirectFetcher`].
pub struct DefaultFetcher {
    proxy: SubscriptionProxy,
    agents: FetchAgents,
}

impl DefaultFetcher {
    /// `proxy_url` is an HTTP proxy endpoint such as `http://127.0.0.1:17890`.
    /// It is used for both URL schemes unless [`Self::with_proxy`] supplies
    /// separate endpoints.
    pub fn new(proxy_url: Option<String>) -> Result<Self, SubscriptionError> {
        let proxy_url = normalize_proxy_endpoint(proxy_url);
        Self::with_proxy(SubscriptionProxy {
            http: proxy_url.clone(),
            https: proxy_url,
            socks: None,
        })
    }

    /// Build a fetcher from per-scheme endpoints.
    pub fn with_proxy(proxy: SubscriptionProxy) -> Result<Self, SubscriptionError> {
        let proxy = proxy.normalized();
        let agents = FetchAgents::from_proxy(&proxy)?;
        Ok(Self { proxy, agents })
    }

    pub fn proxy_url(&self) -> Option<&str> {
        self.proxy
            .http
            .as_deref()
            .or(self.proxy.https.as_deref())
            .or(self.proxy.socks.as_deref())
    }
}

impl HttpFetcher for DefaultFetcher {
    fn bypasses_system_proxy(&self) -> bool {
        self.proxy.is_empty()
    }

    fn get(
        &self,
        url: &str,
        etag: Option<&str>,
        last_modified: Option<&str>,
    ) -> Result<FetchResponse, SubscriptionError> {
        fetch_response(&self.agents, url, etag, last_modified)
    }
}

fn resolve_redirect_url(base: &str, location: &str) -> Result<String, SubscriptionError> {
    let location = location.trim();
    if location.is_empty() {
        return Err(SubscriptionError::FetchFailed(
            "redirect response missing Location".into(),
        ));
    }
    if location.starts_with("http://") || location.starts_with("https://") {
        return Ok(location.to_string());
    }
    url::Url::parse(base)
        .and_then(|b| b.join(location))
        .map(|u| u.to_string())
        .map_err(|e| {
            SubscriptionError::FetchFailed(format!("resolve redirect location {location}: {e}"))
        })
}

fn is_redirect_status(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

fn url_is_https(url: &str) -> bool {
    url::Url::parse(url)
        .ok()
        .is_some_and(|u| u.scheme() == "https")
}

/// Fake-ip DNS (198.18/15) cannot be used as a connect target and must not
/// fall back to an unpinned hostname GET (second resolve skips SSRF checks).
fn refuse_fake_ip_connect(addrs: &[std::net::SocketAddr]) -> Result<(), SubscriptionError> {
    if addrs_are_fake_ip(addrs) {
        Err(SubscriptionError::FetchFailed(
            "subscription DNS resolved only to fake-ip addresses (198.18.0.0/15); refusing an unpinned hostname connect".into(),
        ))
    } else {
        Ok(())
    }
}

fn url_port(url: &str) -> Result<u16, SubscriptionError> {
    let parsed = url::Url::parse(url)
        .map_err(|e| SubscriptionError::FetchFailed(format!("invalid subscription URL: {e}")))?;
    parsed
        .port_or_known_default()
        .ok_or_else(|| SubscriptionError::FetchFailed("subscription URL missing port".into()))
}

fn apply_conditional_headers(
    mut req: ureq::Request,
    hop: u32,
    etag: Option<&str>,
    last_modified: Option<&str>,
) -> Result<ureq::Request, SubscriptionError> {
    if hop == 0 {
        if let Some(etag) = etag {
            req = req.set("If-None-Match", &sanitize_http_header_value(etag)?);
        }
        if let Some(lm) = last_modified {
            req = req.set("If-Modified-Since", &sanitize_http_header_value(lm)?);
        }
    }
    Ok(req)
}

struct FetchAttempt<'a> {
    agent: &'a ureq::Agent,
    connect_url: &'a str,
    host_header: &'a str,
    hop: u32,
    etag: Option<&'a str>,
    last_modified: Option<&'a str>,
    log_url: &'a str,
    via_ip: Option<std::net::IpAddr>,
    /// Time left before the overall fetch deadline.
    timeout: Duration,
}

fn try_request(attempt: FetchAttempt<'_>) -> Result<ureq::Response, SubscriptionError> {
    let req = attempt
        .agent
        .get(attempt.connect_url)
        .timeout(attempt.timeout)
        .set("Host", attempt.host_header);
    let req = apply_conditional_headers(req, attempt.hop, attempt.etag, attempt.last_modified)?;
    req.call().map_err(|e| {
        if let Some(ip) = attempt.via_ip {
            SubscriptionError::FetchFailed(format!("GET {} via {ip}: {e}", attempt.log_url))
        } else {
            SubscriptionError::FetchFailed(format!("GET {}: {e}", attempt.log_url))
        }
    })
}

/// Forward an absolute URL through the configured proxy. The proxy derives
/// the `Host` header (including a non-default port) instead of being pinned to
/// a pre-resolved IP.
fn proxied_request(
    agent: &ureq::Agent,
    url: &str,
    hop: u32,
    etag: Option<&str>,
    last_modified: Option<&str>,
    log_url: &str,
    timeout: Duration,
) -> Result<ureq::Response, SubscriptionError> {
    let req = agent
        .get(url)
        .timeout(timeout)
        .set("User-Agent", "ice-box/0.1")
        .set("Accept", "*/*");
    let req = apply_conditional_headers(req, hop, etag, last_modified)?;
    req.call()
        .map_err(|e| SubscriptionError::FetchFailed(format!("GET {log_url}: {e}")))
}

fn conditional_header_pairs(
    hop: u32,
    etag: Option<&str>,
    last_modified: Option<&str>,
) -> Result<Vec<(&'static str, String)>, SubscriptionError> {
    let mut out = Vec::new();
    if hop == 0 {
        if let Some(v) = etag {
            out.push(("If-None-Match", sanitize_http_header_value(v)?));
        }
        if let Some(v) = last_modified {
            out.push(("If-Modified-Since", sanitize_http_header_value(v)?));
        }
    }
    Ok(out)
}

#[derive(Debug)]
struct HopResponse {
    status: u16,
    etag: Option<String>,
    last_modified: Option<String>,
    userinfo: Option<SubscriptionUserInfo>,
    content_disposition: Option<String>,
    location: Option<String>,
    body: Vec<u8>,
}

fn read_limited_body(reader: impl Read) -> Result<Vec<u8>, SubscriptionError> {
    let mut body = Vec::new();
    reader
        .take(MAX_BODY_BYTES as u64 + 1)
        .read_to_end(&mut body)
        .map_err(|e| SubscriptionError::FetchFailed(format!("read body: {e}")))?;
    if body.len() > MAX_BODY_BYTES {
        return Err(SubscriptionError::FetchFailed(format!(
            "body exceeds {MAX_BODY_BYTES} bytes"
        )));
    }
    Ok(body)
}

fn hop_from_ureq(response: ureq::Response) -> Result<HopResponse, SubscriptionError> {
    let status = response.status();
    Ok(HopResponse {
        status,
        etag: response.header("etag").map(str::to_string),
        last_modified: response.header("last-modified").map(str::to_string),
        userinfo: response
            .header("subscription-userinfo")
            .and_then(parse_userinfo_header),
        content_disposition: response.header("content-disposition").map(str::to_string),
        location: response.header("location").map(str::to_string),
        body: if is_redirect_status(status) || status == 304 {
            Vec::new()
        } else {
            read_limited_body(response.into_reader())?
        },
    })
}

fn hop_from_tls(
    url: &str,
    host_header: &str,
    hop: u32,
    etag: Option<&str>,
    last_modified: Option<&str>,
    ip: std::net::IpAddr,
    deadline: Instant,
) -> Result<HopResponse, SubscriptionError> {
    let port = url_port(url)?;
    let path_query = url_path_query(url)?;
    let headers = conditional_header_pairs(hop, etag, last_modified)?;
    let header_refs: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let resp = tls_get_pinned(
        host_header,
        port,
        ip,
        &path_query,
        &header_refs,
        url,
        deadline,
    )?;
    let status = resp.status;
    Ok(HopResponse {
        status,
        etag: resp.header("etag").map(str::to_string),
        last_modified: resp.header("last-modified").map(str::to_string),
        userinfo: resp
            .header("subscription-userinfo")
            .and_then(parse_userinfo_header),
        content_disposition: resp.header("content-disposition").map(str::to_string),
        location: resp.header("location").map(str::to_string),
        body: if is_redirect_status(status) || status == 304 {
            Vec::new()
        } else {
            resp.body
        },
    })
}

fn handle_hop(
    hop: HopResponse,
    original_url: &str,
    current_url: &str,
    hop_idx: u32,
) -> Result<Result<HopResponse, String>, SubscriptionError> {
    if is_redirect_status(hop.status) {
        if hop_idx == MAX_REDIRECTS {
            return Err(SubscriptionError::FetchFailed(format!(
                "GET {original_url}: too many redirects"
            )));
        }
        let location = hop.location.ok_or_else(|| {
            SubscriptionError::FetchFailed(format!(
                "GET {current_url}: redirect HTTP {} without Location",
                hop.status
            ))
        })?;
        let next = resolve_redirect_url(current_url, &location)?;
        // Never follow an https → http downgrade: the subscription body (which may
        // contain proxy credentials) would travel in cleartext.
        if url_is_https(current_url) && !url_is_https(&next) {
            return Err(SubscriptionError::FetchFailed(format!(
                "GET {current_url}: refusing https→http redirect downgrade to {next}"
            )));
        }
        return Ok(Err(next));
    }
    Ok(Ok(hop))
}

/// GET with manual redirect handling. The direct path re-validates the URL
/// (including DNS) before each hop; the proxy path re-checks its shape and lets
/// the proxy resolve it.
///
/// Direct mode connects to pre-resolved IPs (TLS SNI uses the original
/// hostname). Proxy mode forwards the URL through the configured proxy, which
/// resolves and routes it like any other browser request.
fn fetch_get(
    agents: &FetchAgents,
    url: &str,
    etag: Option<&str>,
    last_modified: Option<&str>,
    deadline: Instant,
) -> Result<HopResponse, SubscriptionError> {
    let mut current = url.to_string();

    for hop in 0..=MAX_REDIRECTS {
        // Every hop shares the one overall deadline, so a redirect chain cannot
        // multiply the wait budget.
        let time_left = fetch_time_left(&current, deadline)?;
        // Pick the proxy per hop: a redirect can change the scheme.
        let (agent, proxied) = agents.for_url(&current);
        if proxied {
            // The proxy resolves the host. A local lookup here would make the
            // fetch fail whenever the local resolver is the broken hop. This is
            // a deliberate SSRF trade-off: direct mode keeps the full DNS guard;
            // proxy mode still rejects literal private hosts, restricted names,
            // and https->http downgrades on every hop (see docs/architecture.md).
            validate_subscription_url_syntax(&current)?;
            let response = proxied_request(
                agent,
                &current,
                hop,
                etag,
                last_modified,
                &current,
                time_left,
            )?;
            let hop_resp = hop_from_ureq(response)?;
            match handle_hop(hop_resp, url, &current, hop)? {
                Ok(final_hop) => return Ok(final_hop),
                Err(next_url) => {
                    current = next_url;
                    continue;
                }
            }
        }

        validate_subscription_url(&current)?;
        let (host_header, addrs) = resolve_allowed_fetch_addrs(&current)?;

        // Fake-ip answers (198.18/15) must not fall back to a hostname
        // connect: ureq would resolve again without the SSRF filter (DNS
        // rebinding). Fail closed and tell the user to use a real resolver.
        refuse_fake_ip_connect(&addrs)?;

        let https = url_is_https(&current);

        let mut hop_err: Option<SubscriptionError> = None;

        for addr in addrs {
            let time_left = fetch_time_left(&current, deadline)?;
            let result = if https {
                hop_from_tls(
                    &current,
                    &host_header,
                    hop,
                    etag,
                    last_modified,
                    addr.ip(),
                    deadline,
                )
            } else {
                let pinned = pin_url_to_ip(&current, addr.ip())?;
                match try_request(FetchAttempt {
                    agent,
                    connect_url: &pinned,
                    host_header: &host_header,
                    hop,
                    etag,
                    last_modified,
                    log_url: &current,
                    via_ip: Some(addr.ip()),
                    timeout: time_left,
                }) {
                    Ok(response) => hop_from_ureq(response),
                    Err(err) => Err(err),
                }
            };

            match result {
                Ok(hop_resp) => match handle_hop(hop_resp, url, &current, hop)? {
                    Ok(final_hop) => return Ok(final_hop),
                    Err(next_url) => {
                        current = next_url;
                        hop_err = None;
                        break;
                    }
                },
                Err(err) => hop_err = Some(err),
            }
        }

        if let Some(err) = hop_err {
            return Err(err);
        }
        if hop == MAX_REDIRECTS {
            return Err(SubscriptionError::FetchFailed(format!(
                "GET {url}: too many redirects"
            )));
        }
    }

    Err(SubscriptionError::FetchFailed(format!(
        "GET {url}: too many redirects"
    )))
}

fn fetch_response(
    agents: &FetchAgents,
    url: &str,
    etag: Option<&str>,
    last_modified: Option<&str>,
) -> Result<FetchResponse, SubscriptionError> {
    fetch_response_with_deadline(
        agents,
        url,
        etag,
        last_modified,
        Instant::now() + FETCH_TOTAL_TIMEOUT,
    )
}

/// [`fetch_response`] with an explicit deadline so tests can use short budgets.
fn fetch_response_with_deadline(
    agents: &FetchAgents,
    url: &str,
    etag: Option<&str>,
    last_modified: Option<&str>,
    deadline: Instant,
) -> Result<FetchResponse, SubscriptionError> {
    let redacted = crate::redact_subscription_url_for_log(url);
    tracing::debug!(url = %redacted, proxied = agents.any(), "subscription fetch: start");
    let hop = fetch_get(agents, url, etag, last_modified, deadline).inspect_err(
        |e| tracing::warn!(url = %redacted, error = %e.redacted_display(), "subscription fetch: failed"),
    )?;
    tracing::debug!(url = %redacted, status = hop.status, bytes = hop.body.len(), "subscription fetch: response");

    if hop.status == 304 {
        return Ok(FetchResponse {
            body: String::new(),
            not_modified: true,
            etag: hop.etag,
            last_modified: hop.last_modified,
            userinfo: hop.userinfo,
            content_disposition: hop.content_disposition,
        });
    }

    if !(200..300).contains(&hop.status) {
        return Err(SubscriptionError::FetchFailed(format!(
            "GET {url}: HTTP {}",
            hop.status
        )));
    }

    let body = String::from_utf8(hop.body)
        .map_err(|e| SubscriptionError::FetchFailed(format!("body not utf-8: {e}")))?;

    Ok(FetchResponse {
        body,
        not_modified: false,
        etag: hop.etag,
        last_modified: hop.last_modified,
        userinfo: hop.userinfo,
        content_disposition: hop.content_disposition,
    })
}

/// ureq client that does **not** use environment proxy settings.
#[derive(Debug, Default, Clone, Copy)]
pub struct DirectFetcher;

impl HttpFetcher for DirectFetcher {
    fn bypasses_system_proxy(&self) -> bool {
        true
    }

    fn get(
        &self,
        url: &str,
        etag: Option<&str>,
        last_modified: Option<&str>,
    ) -> Result<FetchResponse, SubscriptionError> {
        fetch_response(&FetchAgents::direct(), url, etag, last_modified)
    }
}

#[derive(Debug, Clone)]
pub enum MockFetchMode {
    Ok(FetchResponse),
    NotModified,
    Timeout,
    TooLarge,
    Fail(String),
    /// Panic on the first `get` (shared across clones), then behave as `Ok`.
    PanicOnce(PanicOnceMode),
}

/// Payload for [`MockFetchMode::PanicOnce`].
#[derive(Debug, Clone)]
pub struct PanicOnceMode {
    pub response: FetchResponse,
    remaining: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl PanicOnceMode {
    pub fn new(response: FetchResponse) -> Self {
        Self {
            response,
            remaining: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Arm so the next `get` panics; later calls return [`Self::response`].
    pub fn arm(&self) {
        self.remaining
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Mock fetcher for unit tests.
#[derive(Debug, Clone)]
pub struct MockFetcher {
    pub bypasses_proxy: bool,
    pub mode: MockFetchMode,
}

impl HttpFetcher for MockFetcher {
    fn bypasses_system_proxy(&self) -> bool {
        self.bypasses_proxy
    }

    fn get(
        &self,
        _url: &str,
        _etag: Option<&str>,
        _last_modified: Option<&str>,
    ) -> Result<FetchResponse, SubscriptionError> {
        match &self.mode {
            MockFetchMode::Ok(r) => Ok(r.clone()),
            MockFetchMode::NotModified => Ok(FetchResponse {
                body: String::new(),
                not_modified: true,
                etag: None,
                last_modified: None,
                userinfo: None,
                content_disposition: None,
            }),
            MockFetchMode::Timeout => Err(SubscriptionError::FetchFailed(
                "mock timeout after 20s".into(),
            )),
            MockFetchMode::TooLarge => Err(SubscriptionError::FetchFailed(format!(
                "body exceeds {MAX_BODY_BYTES} bytes"
            ))),
            MockFetchMode::Fail(msg) => Err(SubscriptionError::FetchFailed(msg.clone())),
            MockFetchMode::PanicOnce(once) => {
                if once
                    .remaining
                    .swap(false, std::sync::atomic::Ordering::SeqCst)
                {
                    panic!("injected fetch panic");
                }
                Ok(once.response.clone())
            }
        }
    }
}

/// Download an HTTPS resource to `dest`, re-checking SSRF rules on every hop.
/// `allow_host` runs before DNS, so a caller can refuse hosts the subscription
/// fetch would still accept. A rejected host does not open a connection.
pub fn download_direct(
    url: &str,
    dest: &std::path::Path,
    max_bytes: u64,
    allow_host: impl Fn(&str) -> bool,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<(), SubscriptionError> {
    if !url_is_https(url) {
        return Err(SubscriptionError::FetchFailed(
            "update download requires https".into(),
        ));
    }
    let mut current = url.to_string();
    for hop in 0..=MAX_REDIRECTS {
        if !url_is_https(&current) {
            return Err(SubscriptionError::FetchFailed(
                "refusing https→http redirect downgrade".into(),
            ));
        }
        let host = url::Url::parse(&current)
            .ok()
            .and_then(|parsed| parsed.host_str().map(str::to_string))
            .ok_or_else(|| {
                SubscriptionError::FetchFailed("update download URL missing host".into())
            })?;
        if !allow_host(&host) {
            return Err(SubscriptionError::FetchFailed(
                "update download host is not allowed".into(),
            ));
        }
        validate_subscription_url(&current)?;
        let (host_header, addrs) = resolve_allowed_fetch_addrs(&current)?;
        refuse_fake_ip_connect(&addrs)?;
        let port = url_port(&current)?;
        let path_query = crate::tls_fetch::url_path_query(&current)?;

        let mut hop_err: Option<SubscriptionError> = None;
        let mut redirected: Option<String> = None;
        for addr in addrs {
            match crate::tls_fetch::tls_download_pinned(
                crate::tls_fetch::DownloadTarget {
                    host: &host_header,
                    port,
                    ip: addr.ip(),
                    path_query: &path_query,
                    log_url: &current,
                },
                max_bytes,
                dest,
                &mut progress,
            ) {
                Ok(crate::tls_fetch::DownloadHop::Redirect(location)) => {
                    let next = resolve_redirect_url(&current, &location)?;
                    if url_is_https(&current) && !url_is_https(&next) {
                        return Err(SubscriptionError::FetchFailed(
                            "refusing https→http redirect downgrade".into(),
                        ));
                    }
                    redirected = Some(next);
                    hop_err = None;
                    break;
                }
                Ok(crate::tls_fetch::DownloadHop::Saved) => return Ok(()),
                Err(err) => hop_err = Some(err),
            }
        }
        if let Some(next) = redirected {
            if hop == MAX_REDIRECTS {
                return Err(SubscriptionError::FetchFailed(
                    "update download: too many redirects".into(),
                ));
            }
            current = next;
            continue;
        }
        return Err(hop_err
            .unwrap_or_else(|| SubscriptionError::FetchFailed("update download failed".into())));
    }
    Err(SubscriptionError::FetchFailed(
        "update download: too many redirects".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Read the request head up to the blank line. Closing a socket with
    /// unread bytes makes Windows send an RST, which can discard the reply
    /// the fake proxy just wrote, so tests must consume the whole head first.
    fn read_request_head(tcp: &mut std::net::TcpStream) -> String {
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") && head.len() < 8192 {
            match tcp.read(&mut byte) {
                Ok(0) => break,
                Ok(_) => head.push(byte[0]),
                Err(err) => panic!("proxy read: {err}"),
            }
        }
        String::from_utf8_lossy(&head).into_owned()
    }

    #[test]
    fn download_direct_rejects_a_disallowed_host_before_connecting() {
        let dest = std::env::temp_dir().join("ice-box-download-reject-test.apk");
        let _ = std::fs::remove_file(&dest);
        let err = download_direct(
            "https://example.com/app.apk",
            &dest,
            1024,
            |_| false,
            |_, _| {},
        )
        .unwrap_err();
        assert!(err.to_string().contains("not allowed"));
        assert!(!dest.exists());
    }

    #[test]
    fn resolve_redirect_relative_and_absolute() {
        assert_eq!(
            resolve_redirect_url("https://example.com/a/b", "/sub.json").unwrap(),
            "https://example.com/sub.json"
        );
        assert_eq!(
            resolve_redirect_url("https://example.com/a", "https://cdn.example.com/x").unwrap(),
            "https://cdn.example.com/x"
        );
    }

    #[test]
    #[ignore = "network: live HTTPS fetch after SSRF validation"]
    fn https_fetch_succeeds() {
        let agents = FetchAgents::direct();
        let deadline = Instant::now() + FETCH_TOTAL_TIMEOUT;
        let hop = fetch_get(&agents, "https://example.com/", None, None, deadline)
            .expect("HTTPS subscription-style fetch");
        assert_eq!(hop.status, 200);
    }

    #[test]
    #[ignore = "network: live HTTPS fetch through the app proxy (set ICE_BOX_TEST_PROXY)"]
    fn proxied_https_fetch_succeeds() {
        let proxy = std::env::var("ICE_BOX_TEST_PROXY").expect("ICE_BOX_TEST_PROXY");
        let fetcher = DefaultFetcher::new(Some(proxy)).expect("proxy agent");
        let response = fetcher
            .get("https://example.com/", None, None)
            .expect("proxied HTTPS subscription-style fetch");
        assert!(!response.body.is_empty());
    }

    #[test]
    fn fake_ip_only_addrs_do_not_fall_back_to_hostname_connect() {
        let addrs = vec![std::net::SocketAddr::new(
            "198.18.7.3".parse().unwrap(),
            443,
        )];
        let err = refuse_fake_ip_connect(&addrs).unwrap_err();
        assert!(err.to_string().contains("fake-ip"), "got {err}");
        let public = vec![std::net::SocketAddr::new("8.8.8.8".parse().unwrap(), 443)];
        refuse_fake_ip_connect(&public).expect("public IP is not fake-ip");
    }

    #[test]
    fn redirect_to_internal_host_blocked_by_validation() {
        let target = resolve_redirect_url("https://example.com/a", "http://127.0.0.1/x").unwrap();
        let err = validate_subscription_url(&target).unwrap_err();
        assert!(err.to_string().contains("not allowed"), "got {err}");
    }

    #[test]
    fn https_to_http_redirect_downgrade_is_refused() {
        let hop = HopResponse {
            status: 302,
            etag: None,
            last_modified: None,
            userinfo: None,
            content_disposition: None,
            location: Some("http://cdn.example.com/sub".into()),
            body: Vec::new(),
        };
        let err = handle_hop(hop, "https://example.com/a", "https://example.com/a", 0).unwrap_err();
        assert!(err.to_string().contains("https→http"), "got {err}");

        let hop_upgrade = HopResponse {
            status: 302,
            etag: None,
            last_modified: None,
            userinfo: None,
            content_disposition: None,
            location: Some("https://cdn.example.com/sub".into()),
            body: Vec::new(),
        };
        let next = handle_hop(
            hop_upgrade,
            "https://example.com/a",
            "https://example.com/a",
            0,
        )
        .unwrap()
        .unwrap_err();
        assert_eq!(next, "https://cdn.example.com/sub");
    }

    #[test]
    fn https_uses_same_pinned_addr_resolution_as_http() {
        let addrs = resolve_allowed_fetch_addrs("https://127.0.0.1/sub").unwrap_err();
        assert!(addrs.to_string().contains("not allowed"));
    }

    #[test]
    fn conditional_headers_reject_crlf_values() {
        let err = conditional_header_pairs(0, Some("bad\r\nHeader: x"), None).unwrap_err();
        assert!(err.to_string().contains("invalid characters"));
    }

    #[test]
    fn default_fetcher_without_proxy_stays_direct() {
        let fetcher = DefaultFetcher::new(None).unwrap();
        assert!(fetcher.bypasses_system_proxy());
        assert_eq!(fetcher.proxy_url(), None);
    }

    #[test]
    fn proxy_endpoints_follow_the_url_scheme_with_fallbacks() {
        let proxy = SubscriptionProxy {
            http: Some("http://http.example:1".into()),
            https: Some("http://https.example:2".into()),
            socks: Some("socks5://socks.example:3".into()),
        };
        assert_eq!(
            proxy.for_url("http://example.com/sub"),
            Some("http://http.example:1")
        );
        assert_eq!(
            proxy.for_url("https://example.com/sub"),
            Some("http://https.example:2")
        );

        let https_only = SubscriptionProxy {
            http: None,
            https: Some("http://https.example:2".into()),
            socks: None,
        };
        assert_eq!(
            https_only.for_url("http://example.com/sub"),
            Some("http://https.example:2"),
            "a secure-web-only setting still serves plain HTTP"
        );

        let socks_only = SubscriptionProxy {
            http: None,
            https: None,
            socks: Some("socks5://socks.example:3".into()),
        };
        assert_eq!(
            socks_only.for_url("https://example.com/sub"),
            Some("socks5://socks.example:3")
        );
        assert_eq!(
            SubscriptionProxy::default().for_url("https://example.com"),
            None
        );
    }

    #[test]
    fn default_fetcher_proxy_mode_skips_local_dns_and_routes_absolute_urls() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let proxy = std::thread::spawn(move || {
            let (mut tcp, _) = listener.accept().expect("proxy accept");
            tx.send(read_request_head(&mut tcp))
                .expect("record request");
            tcp.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .expect("proxy write");
        });

        let fetcher = DefaultFetcher::new(Some(format!("http://{addr}"))).unwrap();
        assert!(!fetcher.bypasses_system_proxy());
        // `.invalid` never resolves locally; reaching the proxy proves the
        // proxy path does not depend on the local DNS resolver.
        let response = fetcher
            .get("http://does-not-resolve.invalid/sub?t=1", None, None)
            .expect("proxied request");
        assert_eq!(response.body, "ok");
        assert!(!response.not_modified);

        let head = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("proxy saw request");
        assert!(
            head.starts_with("GET http://does-not-resolve.invalid/sub?t=1 HTTP/1.1\r\n"),
            "{head}"
        );
        assert!(
            head.to_ascii_lowercase()
                .contains("\r\nhost: does-not-resolve.invalid\r\n"),
            "{head}"
        );
        proxy.join().expect("proxy thread");
    }

    #[test]
    fn default_fetcher_proxy_mode_still_rejects_private_hosts() {
        let fetcher = DefaultFetcher::new(Some("http://127.0.0.1:1".into())).unwrap();
        for url in [
            "http://127.0.0.1/sub",
            // The proxy would resolve this to loopback; normalize the root dot
            // before applying the restricted-host guard.
            "http://localhost./sub",
        ] {
            let err = fetcher.get(url, None, None).unwrap_err();
            assert!(err.to_string().contains("not allowed"), "{url}: {err}");
        }
    }

    #[test]
    fn default_fetcher_proxy_mode_rejects_private_redirect_targets() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let proxy = std::thread::spawn(move || {
            let (mut tcp, _) = listener.accept().expect("proxy accept");
            let _ = read_request_head(&mut tcp);
            tcp.write_all(
                b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1/secret\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
            .expect("proxy write");
        });

        let fetcher = DefaultFetcher::new(Some(format!("http://{addr}"))).unwrap();
        let err = fetcher
            .get("http://example.com/sub", None, None)
            .expect_err("a redirect to a private host must be rejected");
        assert!(err.to_string().contains("not allowed"), "{err}");
        proxy.join().expect("proxy thread");
    }

    #[test]
    fn fetch_deadline_cuts_off_a_slow_proxy_body() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let server_stop = std::sync::Arc::clone(&stop);
        let server = std::thread::spawn(move || {
            let (mut tcp, _) = listener.accept().expect("proxy accept");
            let _ = read_request_head(&mut tcp);
            tcp.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 100000\r\nConnection: close\r\n\r\n",
            )
            .expect("proxy headers");
            // Drip one byte at a time: every read succeeds inside the
            // per-operation timeout, so only the overall deadline can stop it.
            while !server_stop.load(std::sync::atomic::Ordering::SeqCst) {
                if tcp.write_all(b"x").is_err() {
                    break;
                }
                let _ = tcp.flush();
                std::thread::sleep(Duration::from_millis(20));
            }
        });

        let agents = FetchAgents::from_proxy(&SubscriptionProxy {
            http: Some(format!("http://{addr}")),
            ..SubscriptionProxy::default()
        })
        .unwrap();
        let started = Instant::now();
        let err = fetch_response_with_deadline(
            &agents,
            "http://example.com/sub",
            None,
            None,
            Instant::now() + Duration::from_millis(500),
        )
        .expect_err("a dripping body must not outlive the overall fetch deadline");
        let elapsed = started.elapsed();
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        server.join().expect("proxy thread");

        assert!(elapsed < Duration::from_secs(5), "took {elapsed:?}");
        assert!(err.to_string().contains("timed out"), "{err}");
    }

    #[test]
    fn fetch_time_left_reports_a_timeout_after_the_deadline() {
        let deadline = Instant::now() + Duration::from_millis(20);
        fetch_time_left("http://example.com/sub", deadline).expect("before the deadline");
        std::thread::sleep(Duration::from_millis(50));
        let err = fetch_time_left("http://example.com/sub", deadline).unwrap_err();
        assert!(err.to_string().contains("timed out after 60s"), "{err}");
    }
}
