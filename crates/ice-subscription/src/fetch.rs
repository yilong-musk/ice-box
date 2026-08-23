//! HTTP fetch: direct (no system proxy), 20s timeout, 8 MiB cap.

use std::io::Read;
use std::time::Duration;

use crate::error::SubscriptionError;
use crate::tls_fetch::{tls_get_pinned, url_path_query};
use crate::url::{
    addrs_are_fake_ip, pin_url_to_ip, resolve_allowed_fetch_addrs, validate_subscription_url,
};

/// Hard body size limit (architecture / plan).
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

/// Fetch timeout.
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(20);

/// Max redirect hops; each target is SSRF-validated before follow.
const MAX_REDIRECTS: u32 = 5;

#[derive(Debug, Clone)]
pub struct FetchResponse {
    pub body: String,
    /// HTTP 304 Not Modified — body is empty; caller should keep cached subscription bytes.
    pub not_modified: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_disposition: Option<String>,
}

pub trait HttpFetcher: Send {
    /// When true, client is configured to bypass OS / env proxies (G5.12).
    fn bypasses_system_proxy(&self) -> bool;

    fn get(
        &self,
        url: &str,
        etag: Option<&str>,
        last_modified: Option<&str>,
    ) -> Result<FetchResponse, SubscriptionError>;
}

fn build_direct_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(FETCH_TIMEOUT)
        .timeout_read(FETCH_TIMEOUT)
        .timeout_write(FETCH_TIMEOUT)
        .redirects(0)
        .build()
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
}

fn try_request(attempt: FetchAttempt<'_>) -> Result<ureq::Response, SubscriptionError> {
    let req = attempt
        .agent
        .get(attempt.connect_url)
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
) -> Result<HopResponse, SubscriptionError> {
    let port = url_port(url)?;
    let path_query = url_path_query(url)?;
    let headers = conditional_header_pairs(hop, etag, last_modified)?;
    let header_refs: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let resp = tls_get_pinned(host_header, port, ip, &path_query, &header_refs, url)?;
    let status = resp.status;
    Ok(HopResponse {
        status,
        etag: resp.header("etag").map(str::to_string),
        last_modified: resp.header("last-modified").map(str::to_string),
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

/// GET with manual redirect handling; re-validates URL (incl. DNS) before each hop.
/// Both HTTP and HTTPS connect to pre-resolved IPs (TLS SNI uses the original hostname).
fn fetch_get(
    agent: &ureq::Agent,
    url: &str,
    etag: Option<&str>,
    last_modified: Option<&str>,
) -> Result<HopResponse, SubscriptionError> {
    let mut current = url.to_string();

    for hop in 0..=MAX_REDIRECTS {
        validate_subscription_url(&current)?;
        let (host_header, addrs) = resolve_allowed_fetch_addrs(&current)?;

        // When sing-box / Clash fake-ip DNS is active, pinned HTTP/1.1 to 198.18.x.x returns 403.
        // Route by hostname through the system stack (same path curl uses) instead.
        if addrs_are_fake_ip(&addrs) {
            let req = agent.get(&current);
            let req = apply_conditional_headers(req, hop, etag, last_modified)?;
            let response = req
                .call()
                .map_err(|e| SubscriptionError::FetchFailed(format!("GET {current}: {e}")))?;
            match hop_from_ureq(response) {
                Ok(hop_resp) => match handle_hop(hop_resp, url, &current, hop)? {
                    Ok(final_hop) => return Ok(final_hop),
                    Err(next_url) => {
                        current = next_url;
                        continue;
                    }
                },
                Err(err) => return Err(err),
            }
        }

        let https = url_is_https(&current);
        let _host_header = host_header;

        let mut hop_err: Option<SubscriptionError> = None;

        for addr in addrs {
            let result = if https {
                hop_from_tls(&current, &_host_header, hop, etag, last_modified, addr.ip())
            } else {
                let pinned = pin_url_to_ip(&current, addr.ip())?;
                match try_request(FetchAttempt {
                    agent,
                    connect_url: &pinned,
                    host_header: &_host_header,
                    hop,
                    etag,
                    last_modified,
                    log_url: &current,
                    via_ip: Some(addr.ip()),
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
        let agent = build_direct_agent();
        let hop = fetch_get(&agent, url, etag, last_modified)?;

        if hop.status == 304 {
            return Ok(FetchResponse {
                body: String::new(),
                not_modified: true,
                etag: hop.etag,
                last_modified: hop.last_modified,
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
            content_disposition: hop.content_disposition,
        })
    }
}

#[derive(Debug, Clone)]
pub enum MockFetchMode {
    Ok(FetchResponse),
    NotModified,
    Timeout,
    TooLarge,
    Fail(String),
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
                content_disposition: None,
            }),
            MockFetchMode::Timeout => Err(SubscriptionError::FetchFailed(
                "mock timeout after 20s".into(),
            )),
            MockFetchMode::TooLarge => Err(SubscriptionError::FetchFailed(format!(
                "body exceeds {MAX_BODY_BYTES} bytes"
            ))),
            MockFetchMode::Fail(msg) => Err(SubscriptionError::FetchFailed(msg.clone())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let agent = build_direct_agent();
        let hop = fetch_get(&agent, "https://example.com/", None, None)
            .expect("HTTPS subscription-style fetch");
        assert_eq!(hop.status, 200);
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
}
