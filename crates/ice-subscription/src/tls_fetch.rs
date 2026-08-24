//! Pinned TLS GET (connect to pre-resolved IP, SNI = original hostname).

use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::sync::Arc;

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, RootCertStore};

use crate::error::SubscriptionError;
use crate::fetch::{sanitize_http_header_value, FETCH_TIMEOUT, MAX_BODY_BYTES};

/// Upper bound for status + headers when sizing the raw read buffer.
const MAX_HTTP_HEADER_BYTES: usize = 64 * 1024;

const MAX_RAW_RESPONSE_BYTES: usize = MAX_BODY_BYTES.saturating_add(MAX_HTTP_HEADER_BYTES);

#[derive(Debug)]
pub(crate) struct RawHttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl RawHttpResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

pub(crate) fn url_path_query(url: &str) -> Result<String, SubscriptionError> {
    let parsed = url::Url::parse(url)
        .map_err(|e| SubscriptionError::FetchFailed(format!("invalid subscription URL: {e}")))?;
    let mut path = parsed.path().to_string();
    if path.is_empty() {
        path = "/".to_string();
    }
    if let Some(q) = parsed.query() {
        path.push('?');
        path.push_str(q);
    }
    Ok(path)
}

pub(crate) fn tls_get_pinned(
    host: &str,
    port: u16,
    ip: IpAddr,
    path_query: &str,
    conditional: &[(&str, &str)],
    log_url: &str,
) -> Result<RawHttpResponse, SubscriptionError> {
    let addr = SocketAddr::new(ip, port);
    let mut stream = TcpStream::connect_timeout(&addr, FETCH_TIMEOUT).map_err(|e| {
        SubscriptionError::FetchFailed(format!("GET {log_url} via {ip}: connect: {e}"))
    })?;
    let _ = stream.set_read_timeout(Some(FETCH_TIMEOUT));
    let _ = stream.set_write_timeout(Some(FETCH_TIMEOUT));

    let mut root_store = RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    let server_name = ServerName::try_from(host.to_string()).map_err(|_| {
        SubscriptionError::FetchFailed(format!("GET {log_url}: invalid TLS server name"))
    })?;
    let mut conn = rustls::ClientConnection::new(Arc::new(config), server_name)
        .map_err(|e| SubscriptionError::FetchFailed(format!("GET {log_url} via {ip}: tls: {e}")))?;
    let mut tls = rustls::Stream::new(&mut conn, &mut stream);

    // `Accept` must be present: some subscription frontends WAF-reject requests
    // without it (HTTP 403) regardless of TLS/HTTP version.
    let mut req = format!(
        "GET {path_query} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nUser-Agent: ice-box/0.1\r\nAccept: */*\r\n"
    );
    for (k, v) in conditional {
        let safe = sanitize_http_header_value(v)?;
        req.push_str(&format!("{k}: {safe}\r\n"));
    }
    req.push_str("\r\n");
    tls.write_all(req.as_bytes()).map_err(|e| {
        SubscriptionError::FetchFailed(format!("GET {log_url} via {ip}: write: {e}"))
    })?;

    let mut raw = Vec::new();
    tls.take((MAX_RAW_RESPONSE_BYTES as u64).saturating_add(1))
        .read_to_end(&mut raw)
        .map_err(|e| {
            SubscriptionError::FetchFailed(format!("GET {log_url} via {ip}: read: {e}"))
        })?;
    if raw.len() > MAX_RAW_RESPONSE_BYTES {
        return Err(SubscriptionError::FetchFailed(format!(
            "body exceeds {MAX_BODY_BYTES} bytes"
        )));
    }
    parse_http_response(&raw)
        .map_err(|e| SubscriptionError::FetchFailed(format!("GET {log_url} via {ip}: parse: {e}")))
}

fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn body_too_large() -> String {
    format!("body exceeds {MAX_BODY_BYTES} bytes")
}

fn decode_chunked_body(raw_body: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut rest = raw_body;
    loop {
        let line_end = rest
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or_else(|| "chunked body: missing chunk size line".to_string())?;
        let size_line = std::str::from_utf8(&rest[..line_end])
            .map_err(|e| format!("chunked body: size line not utf-8: {e}"))?;
        let size = usize::from_str_radix(size_line.trim(), 16)
            .map_err(|_| format!("chunked body: invalid chunk size: {size_line}"))?;
        rest = &rest[line_end + 2..];
        if size == 0 {
            break;
        }
        if out.len().saturating_add(size) > MAX_BODY_BYTES {
            return Err(body_too_large());
        }
        if rest.len() < size + 2 {
            return Err("chunked body: truncated chunk data".into());
        }
        out.extend_from_slice(&rest[..size]);
        rest = &rest[size + 2..];
    }
    Ok(out)
}

fn extract_body(raw_body: &[u8], headers: &[(String, String)]) -> Result<Vec<u8>, String> {
    if header_value(headers, "Transfer-Encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked"))
    {
        return decode_chunked_body(raw_body);
    }

    if let Some(cl) = header_value(headers, "Content-Length") {
        let len: usize = cl
            .parse()
            .map_err(|_| format!("invalid Content-Length: {cl}"))?;
        if len > MAX_BODY_BYTES {
            return Err(body_too_large());
        }
        if raw_body.len() < len {
            return Err(format!(
                "response truncated: expected {len} bytes, got {}",
                raw_body.len()
            ));
        }
        return Ok(raw_body[..len].to_vec());
    }

    if raw_body.len() > MAX_BODY_BYTES {
        return Err(body_too_large());
    }
    Ok(raw_body.to_vec())
}

fn parse_http_response(raw: &[u8]) -> Result<RawHttpResponse, String> {
    let header_end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| "missing HTTP header terminator".to_string())?;
    let header_text =
        std::str::from_utf8(&raw[..header_end]).map_err(|e| format!("header not utf-8: {e}"))?;
    let mut lines = header_text.split("\r\n");
    let status_line = lines.next().ok_or("empty response")?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| format!("bad status line: {status_line}"))?;
    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    let raw_body = &raw[(header_end + 4)..];
    let body = extract_body(raw_body, &headers)?;
    Ok(RawHttpResponse {
        status,
        headers,
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_http_response() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello";
        let resp = parse_http_response(raw).unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"hello");
        assert_eq!(resp.header("Content-Length"), Some("5"));
    }

    #[test]
    fn parse_chunked_http_response() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n";
        let resp = parse_http_response(raw).unwrap();
        assert_eq!(resp.body, b"hello");
    }

    #[test]
    fn parse_rejects_oversized_content_length() {
        let cl = (MAX_BODY_BYTES + 1).to_string();
        let raw = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {cl}\r\n\r\n{}",
            "x".repeat(MAX_BODY_BYTES + 1)
        );
        let err = parse_http_response(raw.as_bytes()).unwrap_err();
        assert!(err.contains("exceeds"));
    }

    #[test]
    fn parse_rejects_oversized_connection_close_body() {
        let body = "x".repeat(MAX_BODY_BYTES + 1);
        let raw = format!("HTTP/1.1 200 OK\r\n\r\n{body}");
        let err = parse_http_response(raw.as_bytes()).unwrap_err();
        assert!(err.contains("exceeds"));
    }

    #[test]
    fn sanitize_rejects_crlf_in_header_values() {
        let err = sanitize_http_header_value("etag\r\nInjected: yes").unwrap_err();
        assert!(err.to_string().contains("invalid characters"));
    }

    #[test]
    fn url_path_query_includes_query() {
        assert_eq!(
            url_path_query("https://example.com/a/b?x=1").unwrap(),
            "/a/b?x=1"
        );
    }
}
