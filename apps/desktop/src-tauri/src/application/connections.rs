// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

/// One live connection as the Connections page shows it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectionRow {
    pub id: String,
    /// `tcp` or `udp`.
    pub network: String,
    /// Inbound as `type/tag`, for example `mixed/mixed-in`.
    pub inbound: String,
    /// Client address as `ip:port`; empty when unknown.
    pub source: String,
    /// Destination domain; empty for a bare-IP destination.
    pub host: String,
    /// Destination IP; empty when the destination is a domain.
    pub destination_ip: String,
    pub destination_port: String,
    /// Process path where the core could resolve it; otherwise empty.
    pub process_path: String,
    /// Bytes sent and received so far.
    pub upload: u64,
    pub download: u64,
    /// Creation time in Unix milliseconds; `None` when the core's timestamp
    /// did not parse.
    pub start_ms: Option<i64>,
    /// Outbound chain, exit node first, route target last.
    pub chains: Vec<String>,
    /// Matched route rule with its action, or `final`.
    pub rule: String,
}

/// Live connections of the running core.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct ConnectionsResponse {
    /// False while the core is not running; the list is then empty.
    pub running: bool,
    /// Bytes the core has relayed since it started.
    pub upload_total: u64,
    pub download_total: u64,
    pub connections: Vec<ConnectionRow>,
}

/// Connection count and relayed bytes for the Home strip: the totals of
/// [`ConnectionsResponse`] without the rows, so a once-a-second poll does not
/// carry every connection across the IPC bridge.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct ConnectionStats {
    /// False while the core is not running; the figures are then zero.
    pub running: bool,
    /// Live connections, counted like the Connections page lists them.
    pub count: usize,
    /// Bytes the core has relayed since it started.
    pub upload_total: u64,
    pub download_total: u64,
}

#[derive(Deserialize)]
pub struct CloseConnectionRequest {
    pub id: String,
}

/// Connections the MCP `list_connections` tool returns when it names no limit.
pub(crate) const QUERY_CONNECTIONS_DEFAULT_LIMIT: usize = 200;
/// Upper bound on one `list_connections` result.
pub(crate) const QUERY_CONNECTIONS_MAX_LIMIT: usize = 2000;

/// Filter for the MCP `list_connections` tool.
#[derive(Debug, Default, Deserialize)]
pub struct QueryConnectionsRequest {
    /// Case-insensitive substring of host, IP, port, network, inbound, source,
    /// process, rule or an outbound in the chain.
    #[serde(default)]
    pub keyword: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

/// One bounded, newest-first slice of the live connections.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ConnectionsPage {
    /// False while the core is not running; the list is then empty.
    pub running: bool,
    pub upload_total: u64,
    pub download_total: u64,
    /// Every live connection, before the keyword filter.
    pub total: usize,
    /// Connections the keyword matched, before the limit.
    pub matched: usize,
    pub connections: Vec<ConnectionRow>,
}

/// Row fields a keyword searches. The Connections page searches the same
/// fields (`CONNECTION_SEARCH_FIELDS` in `src/lib/connections.ts`); a test
/// keeps the two lists equal, and another that `row_matches` searches
/// exactly these.
#[cfg(test)]
const SEARCH_FIELDS: [&str; 9] = [
    "host",
    "destination_ip",
    "destination_port",
    "network",
    "inbound",
    "source",
    "process_path",
    "rule",
    "chains",
];

fn row_matches(row: &ConnectionRow, needle: &str) -> bool {
    [
        &row.host,
        &row.destination_ip,
        &row.destination_port,
        &row.network,
        &row.inbound,
        &row.source,
        &row.process_path,
        &row.rule,
    ]
    .into_iter()
    .chain(row.chains.iter())
    .any(|field| field.to_lowercase().contains(needle))
}

fn connections_page(
    response: ConnectionsResponse,
    req: &QueryConnectionsRequest,
) -> ConnectionsPage {
    let total = response.connections.len();
    let needle = req
        .keyword
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .to_lowercase();
    let mut rows: Vec<ConnectionRow> = response
        .connections
        .into_iter()
        .filter(|row| needle.is_empty() || row_matches(row, &needle))
        .collect();
    let matched = rows.len();
    // Newest first; a row without a start time sorts last. The id breaks ties
    // so the order is stable between calls.
    rows.sort_by(|a, b| b.start_ms.cmp(&a.start_ms).then_with(|| a.id.cmp(&b.id)));
    let limit = req
        .limit
        .unwrap_or(QUERY_CONNECTIONS_DEFAULT_LIMIT)
        .clamp(1, QUERY_CONNECTIONS_MAX_LIMIT);
    rows.truncate(limit);
    ConnectionsPage {
        running: response.running,
        upload_total: response.upload_total,
        download_total: response.download_total,
        total,
        matched,
        connections: rows,
    }
}

pub(crate) fn query_connections_use_case(
    state: &AppState,
    req: &QueryConnectionsRequest,
) -> Result<ConnectionsPage, AppError> {
    Ok(connections_page(list_connections_use_case(state)?, req))
}

/// The core writes RFC 3339 with nanoseconds and a local offset. A webview
/// date parser is not guaranteed to take nine fraction digits, so the page
/// gets milliseconds.
fn start_millis(start: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(start)
        .ok()
        .map(|time| time.timestamp_millis())
}

fn connection_row(info: ice_core::ConnectionInfo) -> ConnectionRow {
    ConnectionRow {
        start_ms: start_millis(&info.start),
        id: info.id,
        network: info.network,
        inbound: info.inbound,
        source: info.source,
        host: info.host,
        destination_ip: info.destination_ip,
        destination_port: info.destination_port,
        process_path: info.process_path,
        upload: info.upload,
        download: info.download,
        chains: info.chains,
        rule: info.rule,
    }
}

fn connections_response(snapshot: ice_core::ConnectionsSnapshot) -> ConnectionsResponse {
    ConnectionsResponse {
        running: true,
        upload_total: snapshot.upload_total,
        download_total: snapshot.download_total,
        connections: snapshot
            .connections
            .into_iter()
            .map(connection_row)
            .collect(),
    }
}

fn connection_stats(snapshot: &ice_core::ConnectionsSnapshot) -> ConnectionStats {
    ConnectionStats {
        running: true,
        count: snapshot.connections.len(),
        upload_total: snapshot.upload_total,
        download_total: snapshot.download_total,
    }
}

/// The running core's Clash API. The traffic stream already follows it, so a
/// poll reads no files and keeps the port the core started with even after
/// `settings.json` changes. Settings are the fallback for a running core
/// whose stream is not attached.
fn live_endpoints(state: &AppState) -> Result<ice_core::HealthEndpoints, AppError> {
    if let Some(endpoints) = state.traffic.endpoints() {
        return Ok(endpoints);
    }
    let settings = current_settings(&state.paths)?;
    clash_endpoints(&state.paths, &settings)
}

/// A stopped core has no connections; that is a state of the page, not an
/// error.
pub(crate) fn list_connections_use_case(state: &AppState) -> Result<ConnectionsResponse, AppError> {
    if state.core_snapshot.load().state.status != CoreStatus::Running {
        return Ok(ConnectionsResponse::default());
    }
    let endpoints = live_endpoints(state)?;
    let snapshot = ice_core::connections(&endpoints).map_err(AppError::from)?;
    Ok(connections_response(snapshot))
}

/// Same source and stopped-core rule as [`list_connections_use_case`].
pub(crate) fn connection_stats_use_case(state: &AppState) -> Result<ConnectionStats, AppError> {
    if state.core_snapshot.load().state.status != CoreStatus::Running {
        return Ok(ConnectionStats::default());
    }
    let endpoints = live_endpoints(state)?;
    let snapshot = ice_core::connections(&endpoints).map_err(AppError::from)?;
    Ok(connection_stats(&snapshot))
}

pub(crate) fn close_connection_use_case(
    state: &AppState,
    req: CloseConnectionRequest,
) -> Result<(), AppError> {
    // The id becomes a URL path segment. The core issues UUIDs; anything
    // else did not come from its list.
    if Uuid::parse_str(&req.id).is_err() {
        return Err(AppError::new(
            ErrorCode::ConfigInvalid,
            "connection id is not a UUID",
        ));
    }
    require_running_core(state)?;
    let endpoints = live_endpoints(state)?;
    ice_core::close_connection(&endpoints, &req.id).map_err(AppError::from)
}

pub(crate) fn close_all_connections_use_case(state: &AppState) -> Result<(), AppError> {
    require_running_core(state)?;
    let endpoints = live_endpoints(state)?;
    ice_core::close_all_connections(&endpoints).map_err(AppError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_time_becomes_unix_milliseconds() {
        // Nanosecond fraction and a local offset, as the core writes it.
        assert_eq!(
            start_millis("2026-11-10T11:40:00.123456789+08:00"),
            Some(1_794_282_000_123)
        );
        assert_eq!(
            start_millis("2026-11-10T03:40:00Z"),
            Some(1_794_282_000_000)
        );
        assert_eq!(start_millis(""), None);
        assert_eq!(start_millis("yesterday"), None);
    }

    fn sample_row(id: &str, host: &str, start_ms: Option<i64>) -> ConnectionRow {
        ConnectionRow {
            id: id.into(),
            network: "tcp".into(),
            inbound: "mixed/mixed-in".into(),
            source: "127.0.0.1:51234".into(),
            host: host.into(),
            destination_ip: String::new(),
            destination_port: "443".into(),
            process_path: String::new(),
            upload: 0,
            download: 0,
            start_ms,
            chains: vec!["node-a".into(), "proxy".into()],
            rule: "final".into(),
        }
    }

    fn sample_response() -> ConnectionsResponse {
        ConnectionsResponse {
            running: true,
            upload_total: 1,
            download_total: 2,
            connections: vec![
                sample_row("b", "old.example", Some(1_000)),
                sample_row("c", "api.github.com", Some(3_000)),
                sample_row("a", "no-start.example", None),
                sample_row("d", "new.github.com", Some(3_000)),
            ],
        }
    }

    fn ids(page: &ConnectionsPage) -> Vec<&str> {
        page.connections.iter().map(|row| row.id.as_str()).collect()
    }

    #[test]
    fn page_lists_newest_first_with_a_stable_tie_break() {
        let page = connections_page(sample_response(), &QueryConnectionsRequest::default());
        assert_eq!(ids(&page), vec!["c", "d", "b", "a"]);
        assert_eq!((page.total, page.matched), (4, 4));
        assert!(page.running);
        assert_eq!((page.upload_total, page.download_total), (1, 2));
    }

    #[test]
    fn page_filters_by_keyword_without_case() {
        let page = connections_page(
            sample_response(),
            &QueryConnectionsRequest {
                keyword: Some("  GitHub ".into()),
                limit: None,
            },
        );
        assert_eq!(ids(&page), vec!["c", "d"]);
        assert_eq!((page.total, page.matched), (4, 2));
        let by_chain = connections_page(
            sample_response(),
            &QueryConnectionsRequest {
                keyword: Some("node-a".into()),
                limit: None,
            },
        );
        assert_eq!(by_chain.matched, 4);
        let none = connections_page(
            sample_response(),
            &QueryConnectionsRequest {
                keyword: Some("udp".into()),
                limit: None,
            },
        );
        assert!(none.connections.is_empty());
        assert_eq!(none.matched, 0);
    }

    #[test]
    fn keyword_searches_exactly_the_listed_fields() {
        let needle = "zz-needle";
        let base = serde_json::to_value(sample_row("id", "", Some(1))).expect("json");
        for field in SEARCH_FIELDS {
            let mut value = base.clone();
            value[field] = if field == "chains" {
                serde_json::json!([needle])
            } else {
                serde_json::json!(needle)
            };
            let row: ConnectionRow = serde_json::from_value(value).expect("row");
            assert!(row_matches(&row, needle), "{field} is searched");
        }
        for (field, original) in base.as_object().expect("object") {
            if SEARCH_FIELDS.contains(&field.as_str()) || !original.is_string() {
                continue;
            }
            let mut value = base.clone();
            value[field] = serde_json::json!(needle);
            let row: ConnectionRow = serde_json::from_value(value).expect("row");
            assert!(!row_matches(&row, needle), "{field} is not searched");
        }
    }

    #[test]
    fn page_search_fields_match_rust() {
        let ts = include_str!("../../../src/lib/connections.ts");
        let start = ts
            .find("export const CONNECTION_SEARCH_FIELDS = [")
            .expect("CONNECTION_SEARCH_FIELDS in src/lib/connections.ts");
        let rest = &ts[start..];
        let list = &rest[rest.find('[').expect("[") + 1..rest.find(']').expect("]")];
        let from_ts: Vec<&str> = list
            .split(',')
            .map(|item| item.trim().trim_matches('"'))
            .filter(|item| !item.is_empty())
            .collect();
        assert_eq!(
            from_ts, SEARCH_FIELDS,
            "SEARCH_FIELDS and CONNECTION_SEARCH_FIELDS in src/lib/connections.ts must list the same fields"
        );
    }

    #[test]
    fn page_applies_the_limit_after_matching() {
        let page = connections_page(
            sample_response(),
            &QueryConnectionsRequest {
                keyword: None,
                limit: Some(2),
            },
        );
        assert_eq!(ids(&page), vec!["c", "d"]);
        assert_eq!(page.matched, 4);
        let floor = connections_page(
            sample_response(),
            &QueryConnectionsRequest {
                keyword: None,
                limit: Some(0),
            },
        );
        assert_eq!(floor.connections.len(), 1);
    }

    #[test]
    fn rows_keep_the_core_fields() {
        let snapshot = ice_core::ConnectionsSnapshot {
            upload_total: 10,
            download_total: 20,
            connections: vec![ice_core::ConnectionInfo {
                id: "7a5f6f0e-6c1b-4f0e-9d0a-000000000001".into(),
                network: "tcp".into(),
                inbound: "mixed/mixed-in".into(),
                source: "127.0.0.1:51234".into(),
                host: "api.github.com".into(),
                destination_ip: String::new(),
                destination_port: "443".into(),
                process_path: "/usr/bin/curl".into(),
                upload: 1,
                download: 2,
                start: "2026-11-10T03:40:00Z".into(),
                chains: vec!["node-a".into(), "proxy".into()],
                rule: "final".into(),
            }],
        };
        assert_eq!(
            connection_stats(&snapshot),
            ConnectionStats {
                running: true,
                count: 1,
                upload_total: 10,
                download_total: 20,
            }
        );
        let response = connections_response(snapshot);
        assert!(response.running);
        assert_eq!((response.upload_total, response.download_total), (10, 20));
        let row = &response.connections[0];
        assert_eq!(row.host, "api.github.com");
        assert_eq!(row.start_ms, Some(1_794_282_000_000));
        assert_eq!(row.chains, vec!["node-a", "proxy"]);
        let json = serde_json::to_value(row).expect("json");
        assert_eq!(json["destination_port"], "443");
        assert_eq!(json["process_path"], "/usr/bin/curl");
    }
}
