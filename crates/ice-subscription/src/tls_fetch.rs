// SPDX-License-Identifier: GPL-3.0-or-later

//! Pinned TLS GET (connect to pre-resolved IP, SNI = original hostname).

use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::sync::{Arc, OnceLock};

use flate2::read::GzDecoder;
use rustls::pki_types::ServerName;
use rustls::ClientConfig;
use rustls_platform_verifier::ConfigVerifierExt;

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

fn ensure_crypto_provider() {
    static INSTALLED: OnceLock<()> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// Process-wide rustls client config using the OS trust store (SUB-3).
fn tls_client_config() -> Result<Arc<ClientConfig>, SubscriptionError> {
    static CONFIG: OnceLock<Arc<ClientConfig>> = OnceLock::new();
    if let Some(config) = CONFIG.get() {
        return Ok(Arc::clone(config));
    }
    ensure_crypto_provider();
    let built = ClientConfig::with_platform_verifier()
        .map_err(|e| SubscriptionError::FetchFailed(format!("TLS platform verifier: {e}")))?;
    Ok(Arc::clone(CONFIG.get_or_init(|| Arc::new(built))))
}

pub(crate) fn tls_get_pinned(
    host: &str,
    port: u16,
    ip: IpAddr,
    path_query: &str,
    conditional: &[(&str, &str)],
    log_url: &str,
) -> Result<RawHttpResponse, SubscriptionError> {
    tls_get_pinned_with_config(
        host,
        port,
        ip,
        path_query,
        conditional,
        log_url,
        tls_client_config()?,
    )
}

fn tls_get_pinned_with_config(
    host: &str,
    port: u16,
    ip: IpAddr,
    path_query: &str,
    conditional: &[(&str, &str)],
    log_url: &str,
    config: Arc<ClientConfig>,
) -> Result<RawHttpResponse, SubscriptionError> {
    let addr = SocketAddr::new(ip, port);
    let mut stream = TcpStream::connect_timeout(&addr, FETCH_TIMEOUT).map_err(|e| {
        SubscriptionError::FetchFailed(format!("GET {log_url} via {ip}: connect: {e}"))
    })?;
    let _ = stream.set_read_timeout(Some(FETCH_TIMEOUT));
    let _ = stream.set_write_timeout(Some(FETCH_TIMEOUT));

    let server_name = ServerName::try_from(host.to_string()).map_err(|_| {
        SubscriptionError::FetchFailed(format!("GET {log_url}: invalid TLS server name"))
    })?;
    let mut conn = rustls::ClientConnection::new(config, server_name)
        .map_err(|e| SubscriptionError::FetchFailed(format!("GET {log_url} via {ip}: tls: {e}")))?;
    let mut tls = rustls::Stream::new(&mut conn, &mut stream);

    // `Accept` must be present: some subscription frontends WAF-reject requests
    // without it (HTTP 403) regardless of TLS/HTTP version.
    // `Accept-Encoding: gzip, identity` plus gzip decode (below) covers
    // servers that ignore identity-only requests.
    let mut req = format!(
        "GET {path_query} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nUser-Agent: ice-box/0.1\r\nAccept: */*\r\nAccept-Encoding: gzip, identity\r\n"
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

pub(crate) enum DownloadHop {
    Redirect(String),
    Saved,
}

pub(crate) struct DownloadTarget<'a> {
    pub host: &'a str,
    pub port: u16,
    pub ip: IpAddr,
    pub path_query: &'a str,
    pub log_url: &'a str,
}

/// HTTPS GET that writes a successful body to `dest`. Redirects return the
/// Location and do not create the file. The body cap is the caller's, so a
/// release APK can exceed the subscription fetch limit without raising it.
pub(crate) fn tls_download_pinned(
    target: DownloadTarget<'_>,
    max_bytes: u64,
    dest: &std::path::Path,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<DownloadHop, SubscriptionError> {
    let DownloadTarget {
        host,
        port,
        ip,
        path_query,
        log_url,
    } = target;
    let fail = |detail: String| {
        SubscriptionError::FetchFailed(format!("GET {log_url} via {ip}: {detail}"))
    };
    let addr = SocketAddr::new(ip, port);
    let mut stream = TcpStream::connect_timeout(&addr, FETCH_TIMEOUT)
        .map_err(|e| fail(format!("connect: {e}")))?;
    let _ = stream.set_read_timeout(Some(FETCH_TIMEOUT));
    let _ = stream.set_write_timeout(Some(FETCH_TIMEOUT));

    let server_name = ServerName::try_from(host.to_string())
        .map_err(|_| fail("invalid TLS server name".into()))?;
    let config = tls_client_config()?;
    let mut conn = rustls::ClientConnection::new(config, server_name)
        .map_err(|e| fail(format!("tls: {e}")))?;
    let mut tls = rustls::Stream::new(&mut conn, &mut stream);

    let req = format!(
        "GET {path_query} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nUser-Agent: ice-box/0.1\r\nAccept: */*\r\nAccept-Encoding: identity\r\n\r\n"
    );
    tls.write_all(req.as_bytes())
        .map_err(|e| fail(format!("write: {e}")))?;

    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    let header_end = loop {
        if buf.len() > MAX_HTTP_HEADER_BYTES {
            return Err(fail("HTTP headers are too large".into()));
        }
        let n = tls.read(&mut tmp).map_err(|e| fail(format!("read: {e}")))?;
        if n == 0 {
            return Err(fail("response ended before HTTP headers".into()));
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos;
        }
    };
    let prefetched = buf.split_off(header_end + 4);
    let header_text =
        std::str::from_utf8(&buf).map_err(|e| fail(format!("header not utf-8: {e}")))?;
    let mut lines = header_text.split("\r\n");
    let status_line = lines.next().unwrap_or("");
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|part| part.parse().ok())
        .ok_or_else(|| fail(format!("bad status line: {status_line}")))?;
    let mut headers = Vec::new();
    for line in lines {
        if let Some((key, value)) = line.split_once(':') {
            headers.push((key.trim().to_string(), value.trim().to_string()));
        }
    }
    let header = |name: &str| -> Option<&str> {
        headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    };

    if matches!(status, 301 | 302 | 303 | 307 | 308) {
        let location = header("location")
            .ok_or_else(|| fail(format!("redirect HTTP {status} without Location")))?;
        return Ok(DownloadHop::Redirect(location.to_string()));
    }
    if !(200..300).contains(&status) {
        return Err(fail(format!("HTTP {status}")));
    }
    if header("Transfer-Encoding").is_some_and(|value| value.eq_ignore_ascii_case("chunked")) {
        return Err(fail("chunked update downloads are not supported".into()));
    }
    let content_length = match header("Content-Length") {
        Some(raw) => {
            let len: u64 = raw
                .parse()
                .map_err(|_| fail(format!("invalid Content-Length: {raw}")))?;
            if len == 0 || len > max_bytes {
                return Err(fail(format!("body exceeds {max_bytes} bytes")));
            }
            Some(len)
        }
        None => None,
    };

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| fail(format!("create download dir: {e}")))?;
    }
    let mut file =
        std::fs::File::create(dest).map_err(|e| fail(format!("create download: {e}")))?;
    let mut written: u64 = 0;
    let take = match content_length {
        Some(len) => prefetched.len().min(len as usize),
        None => prefetched.len(),
    };
    if let Err(detail) = append_download_chunk(
        &mut file,
        &mut written,
        &prefetched[..take],
        max_bytes,
        content_length,
        progress,
    ) {
        let _ = std::fs::remove_file(dest);
        return Err(fail(detail));
    }
    loop {
        if content_length.is_some_and(|len| written >= len) {
            break;
        }
        let n = match tls.read(&mut tmp) {
            Ok(n) => n,
            Err(err) => {
                let _ = std::fs::remove_file(dest);
                return Err(fail(format!("read: {err}")));
            }
        };
        if n == 0 {
            break;
        }
        let take = match content_length {
            Some(len) => n.min((len - written) as usize),
            None => n,
        };
        if let Err(detail) = append_download_chunk(
            &mut file,
            &mut written,
            &tmp[..take],
            max_bytes,
            content_length,
            progress,
        ) {
            let _ = std::fs::remove_file(dest);
            return Err(fail(detail));
        }
    }
    if content_length.is_some_and(|len| written != len) || written == 0 {
        let _ = std::fs::remove_file(dest);
        return Err(fail("response truncated".into()));
    }
    if let Err(err) = file.sync_all() {
        let _ = std::fs::remove_file(dest);
        return Err(fail(format!("sync download: {err}")));
    }
    Ok(DownloadHop::Saved)
}

fn append_download_chunk(
    file: &mut std::fs::File,
    written: &mut u64,
    chunk: &[u8],
    max_bytes: u64,
    content_length: Option<u64>,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<(), String> {
    if chunk.is_empty() {
        return Ok(());
    }
    let next = written.saturating_add(chunk.len() as u64);
    if next > max_bytes || content_length.is_some_and(|len| next > len) {
        return Err(format!("body exceeds {max_bytes} bytes"));
    }
    file.write_all(chunk)
        .map_err(|e| format!("write download: {e}"))?;
    *written = next;
    progress(*written, content_length);
    Ok(())
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

fn decode_content_encoding(body: Vec<u8>, headers: &[(String, String)]) -> Result<Vec<u8>, String> {
    let Some(raw) = header_value(headers, "Content-Encoding") else {
        return Ok(body);
    };
    let encoding = raw.split(',').next().unwrap_or(raw).trim();
    if encoding.is_empty() || encoding.eq_ignore_ascii_case("identity") {
        return Ok(body);
    }
    if encoding.eq_ignore_ascii_case("gzip") || encoding.eq_ignore_ascii_case("x-gzip") {
        let mut decoder = GzDecoder::new(body.as_slice()).take(MAX_BODY_BYTES as u64 + 1);
        let mut out = Vec::new();
        decoder
            .read_to_end(&mut out)
            .map_err(|e| format!("gzip decode: {e}"))?;
        if out.len() > MAX_BODY_BYTES {
            return Err(body_too_large());
        }
        return Ok(out);
    }
    Err(format!("unsupported Content-Encoding: {encoding}"))
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
    let body = if body.is_empty() || matches!(status, 100..=199 | 204 | 304) {
        body
    } else {
        decode_content_encoding(body, &headers)?
    };
    Ok(RawHttpResponse {
        status,
        headers,
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;

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
    fn parse_gzip_http_response() {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(br#"{"ok":true}"#).unwrap();
        let gz = encoder.finish().unwrap();
        let mut raw = format!(
            "HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\n\r\n",
            gz.len()
        )
        .into_bytes();
        raw.extend_from_slice(&gz);
        let resp = parse_http_response(&raw).unwrap();
        assert_eq!(resp.body, br#"{"ok":true}"#);
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

    #[test]
    fn tls_client_config_is_cached() {
        let a = tls_client_config().expect("platform verifier");
        let b = tls_client_config().expect("platform verifier");
        assert!(Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn fetch_gzip_body_from_private_ca_tls_server() {
        ensure_crypto_provider();
        let certified = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let cert_der = rustls::pki_types::CertificateDer::from(certified.cert.der().to_vec());
        let key_der = rustls::pki_types::PrivateKeyDer::Pkcs8(
            rustls::pki_types::PrivatePkcs8KeyDer::from(certified.key_pair.serialize_der()),
        );
        let server_config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert_der.clone()], key_der)
            .expect("server cert");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(false).unwrap();
        let addr = listener.local_addr().unwrap();

        let mut body = Vec::new();
        {
            let mut encoder = GzEncoder::new(&mut body, Compression::default());
            encoder.write_all(br#"{"outbounds":[]}"#).unwrap();
            encoder.finish().unwrap();
        }
        let response = {
            let mut raw = format!(
                "HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .into_bytes();
            raw.extend_from_slice(&body);
            raw
        };

        let server = std::thread::spawn(move || {
            let (mut tcp, _) = listener.accept().expect("accept");
            let mut conn =
                rustls::ServerConnection::new(Arc::new(server_config)).expect("server conn");
            {
                let mut tls = rustls::Stream::new(&mut conn, &mut tcp);
                let mut req = [0u8; 1024];
                let _ = tls.read(&mut req);
                tls.write_all(&response).expect("write");
                tls.flush().expect("flush");
            }
            // rustls clients treat TCP EOF without close_notify as an error.
            conn.send_close_notify();
            while conn.wants_write() {
                conn.write_tls(&mut tcp).expect("close_notify");
            }
        });

        let mut roots = rustls::RootCertStore::empty();
        roots.add(cert_der).expect("trust test CA");
        let client = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let resp = tls_get_pinned_with_config(
            "localhost",
            addr.port(),
            addr.ip(),
            "/",
            &[],
            "https://localhost/",
            Arc::new(client),
        )
        .expect("pinned GET");
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, br#"{"outbounds":[]}"#);
        server.join().expect("server");
    }

    #[test]
    fn parse_skips_gzip_on_empty_not_modified() {
        let raw = b"HTTP/1.1 304 Not Modified\r\nContent-Encoding: gzip\r\n\r\n";
        let resp = parse_http_response(raw).unwrap();
        assert_eq!(resp.status, 304);
        assert!(resp.body.is_empty());
    }
}
