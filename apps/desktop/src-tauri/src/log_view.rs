//! Merged user-facing log view.
//!
//! Display-only concern: merges the app log (ice-box.log, `tracing` format) and the
//! core log (sing-box.log, sing-box format) into one time-ordered view, and keeps only
//! lines with user value (WARN/ERROR/FATAL plus key lifecycle INFO and per-connection
//! outbound routing INFO). Connection lines render as `LEVEL TIME TARGET → NODE`.
//! Raw log files are never modified — full recording (info/debug/trace) is untouched
//! for troubleshooting; the filter applies only at read/display time (architecture §16).

use std::path::Path;

use chrono::{DateTime, FixedOffset, NaiveDateTime};

use ice_config::AppError;

use crate::log_tail::{read_log_tail_deep, LOG_TAIL_MAX};

/// Lines scanned per source so filtering still yields up to `VIEW_MAX` lines.
const SCAN_PER_SOURCE: usize = 3000;
const VIEW_MAX: usize = LOG_TAIL_MAX;

/// Core INFO lines worth showing: lifecycle events plus per-connection outbound
/// routing lines (`outbound/<type>[<tag>]: outbound connection to <host>:<port>`),
/// which show which node each connection actually egresses through.
const CORE_INFO_KEYWORDS: &[&str] = &[
    "started",
    "stopped",
    "ready",
    "reload",
    "restart",
    "outbound connection to",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Level {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Fatal,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Source {
    App,
    Core,
}

struct LogLine {
    ts: DateTime<FixedOffset>,
    source: Source,
    level: Level,
    text: String,
}

impl Level {
    fn as_str(self) -> &'static str {
        match self {
            Level::Trace => "TRACE",
            Level::Debug => "DEBUG",
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
            Level::Fatal => "FATAL",
        }
    }
}

fn parse_level(tok: &str) -> Option<Level> {
    match tok {
        "TRACE" => Some(Level::Trace),
        "DEBUG" => Some(Level::Debug),
        "INFO" => Some(Level::Info),
        "WARN" => Some(Level::Warn),
        "ERROR" => Some(Level::Error),
        "FATAL" | "PANIC" => Some(Level::Fatal),
        _ => None,
    }
}

/// Parse a `tracing` fmt line: `2026-08-23T13:47:01.123456Z  INFO target: message`.
fn parse_app_line(line: &str) -> Option<(DateTime<FixedOffset>, Level)> {
    let mut it = line.split_whitespace();
    let ts = it.next()?;
    let lvl = it.next()?;
    let dt = DateTime::parse_from_rfc3339(ts).ok()?;
    let level = parse_level(lvl)?;
    Some((dt, level))
}

fn parse_tz_offset(tok: &str) -> Option<i32> {
    let bytes = tok.as_bytes();
    if bytes.len() != 5 || (bytes[0] != b'+' && bytes[0] != b'-') {
        return None;
    }
    let sign = if bytes[0] == b'-' { -1 } else { 1 };
    let h: i32 = std::str::from_utf8(&bytes[1..3]).ok()?.parse().ok()?;
    let m: i32 = std::str::from_utf8(&bytes[3..5]).ok()?.parse().ok()?;
    if h > 23 || m > 59 {
        return None;
    }
    Some(sign * (h * 3600 + m * 60))
}

/// Parse a sing-box line: `+0800 2026-08-23 13:47:01 INFO message` (or, defensively,
/// `2026-08-23 13:47:01 INFO message` without the zone prefix).
fn parse_core_line(line: &str) -> Option<(DateTime<FixedOffset>, Level)> {
    let toks: Vec<&str> = line.split_whitespace().collect();
    if toks.len() < 4 {
        return None;
    }
    let (date_idx, time_idx) = if toks[1].len() == 10 && toks[2].len() == 8 {
        (1, 2)
    } else if toks[0].len() == 10 && toks[1].len() == 8 {
        (0, 1)
    } else {
        return None;
    };
    let date = toks[date_idx];
    let time = toks[time_idx];
    if date.as_bytes().get(4) != Some(&b'-') {
        return None;
    }
    let offset = if date_idx == 0 {
        FixedOffset::east_opt(0)
    } else {
        FixedOffset::east_opt(parse_tz_offset(toks[0])?).or_else(|| FixedOffset::east_opt(0))
    }?;
    let naive =
        NaiveDateTime::parse_from_str(&format!("{date}T{time}"), "%Y-%m-%dT%H:%M:%S").ok()?;
    let dt = naive.and_local_timezone(offset).single()?;
    let level = parse_level(toks[time_idx + 1])?;
    Some((dt, level))
}

/// Keep only lines with user value; DEBUG/TRACE and core connection noise are hidden.
/// `text` is the original file line (used for INFO keyword matching).
fn display_worthy(source: Source, level: Level, text: &str) -> bool {
    match level {
        Level::Warn | Level::Error | Level::Fatal => true,
        Level::Info => match source {
            // App INFO lines are written deliberately (lifecycle events); keep all.
            Source::App => true,
            Source::Core => {
                let lower = text.to_lowercase();
                CORE_INFO_KEYWORDS.iter().any(|k| lower.contains(k))
            }
        },
        Level::Trace | Level::Debug => false,
    }
}

fn collect(out: &mut Vec<LogLine>, source: Source, path: &Path) -> Result<(), AppError> {
    for raw in read_log_tail_deep(path, SCAN_PER_SOURCE)? {
        let parsed = match source {
            Source::App => parse_app_line(&raw),
            Source::Core => parse_core_line(&raw),
        };
        let Some((ts, level)) = parsed else { continue };
        if display_worthy(source, level, &raw) {
            out.push(LogLine {
                ts,
                source,
                level,
                text: raw,
            });
        }
    }
    Ok(())
}

const CONNECTION_MARK: &str = ": outbound connection to ";

/// Parse sing-box `outbound/<type>[<tag>]: outbound connection to <host>:<port>`.
fn parse_connection_route(text: &str) -> Option<(&str, &str)> {
    let idx = text.find(CONNECTION_MARK)?;
    let head = &text[..idx];
    let target = text[idx + CONNECTION_MARK.len()..].trim();
    if target.is_empty() {
        return None;
    }
    let outbound = match head.rfind("outbound/") {
        Some(pos) => &head[pos + "outbound/".len()..],
        None => return None,
    };
    let node = if let Some(open) = outbound.find('[') {
        let inner = &outbound[open + 1..];
        inner.strip_suffix(']').unwrap_or(inner).trim()
    } else {
        outbound.trim()
    };
    if node.is_empty() {
        return None;
    }
    Some((target, node))
}

/// Display-only simplification for non-connection core lines: drop the `±HHMM`
/// zone token and the per-connection `[<id> <ms>]` prefix.
fn simplify_core_line(text: &str) -> String {
    let mut toks: Vec<&str> = text.split_whitespace().collect();

    if toks.first().is_some_and(|t| is_zone_token(t)) {
        toks.remove(0);
    }

    let mut i = 0;
    while i + 1 < toks.len() {
        let id = toks[i];
        let dur = toks[i + 1];
        let id_ok =
            id.len() > 1 && id.starts_with('[') && id[1..].chars().all(|c| c.is_ascii_digit());
        let body = dur.strip_suffix(']').unwrap_or(dur);
        let dur_ok = body
            .strip_suffix("ms")
            .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit() || c == '.'));
        if id_ok && dur_ok {
            toks.drain(i..=i + 1);
            break;
        }
        i += 1;
    }

    toks.join(" ")
}

fn strip_leading_level(text: &str) -> &str {
    let rest = text.trim_start();
    match rest.split_once(char::is_whitespace) {
        Some((tok, tail)) if parse_level(tok).is_some() => tail.trim_start(),
        _ => rest,
    }
}

fn format_short_ts(ts: DateTime<FixedOffset>) -> String {
    ts.format("%m-%d %H:%M:%S").to_string()
}

fn skip_first_ws_token(text: &str) -> &str {
    match text.trim_start().split_once(char::is_whitespace) {
        Some((_, tail)) => tail.trim_start(),
        None => text.trim_start(),
    }
}

fn is_zone_token(tok: &str) -> bool {
    tok.len() == 5
        && (tok.starts_with('+') || tok.starts_with('-'))
        && tok[1..].chars().all(|c| c.is_ascii_digit())
}

fn is_ymd_token(tok: &str) -> bool {
    tok.len() == 10 && tok.as_bytes().get(4) == Some(&b'-') && tok.as_bytes().get(7) == Some(&b'-')
}

fn is_hms_token(tok: &str) -> bool {
    tok.len() == 8 && tok.as_bytes().get(2) == Some(&b':') && tok.as_bytes().get(5) == Some(&b':')
}

/// Drop original timestamp tokens (RFC3339, `YYYY-MM-DD HH:MM:SS`, optional `±HHMM`).
fn strip_leading_timestamp(text: &str) -> &str {
    let mut rest = text.trim_start();
    if rest.split_whitespace().next().is_some_and(is_zone_token) {
        rest = skip_first_ws_token(rest);
    }
    if rest.split_whitespace().next().is_some_and(is_ymd_token) {
        rest = skip_first_ws_token(rest);
    }
    if rest
        .split_whitespace()
        .next()
        .is_some_and(|t| is_hms_token(t) || t.contains('T'))
    {
        rest = skip_first_ws_token(rest);
    }
    rest
}

/// Compact display line: `LEVEL MM-DD HH:MM:SS …` (no `[app]`/`[core]` tag).
/// Connection lines are `LEVEL TIME TARGET → NODE`; everything else is `LEVEL TIME message`.
fn format_display_line(
    ts: DateTime<FixedOffset>,
    source: Source,
    level: Level,
    raw: &str,
) -> String {
    let time = format_short_ts(ts);
    let lvl = level.as_str();
    if source == Source::Core {
        if let Some((target, node)) = parse_connection_route(raw) {
            return format!("{lvl} {time} {target} → {node}");
        }
    }
    let rendered = match source {
        Source::Core => simplify_core_line(raw),
        Source::App => raw.to_string(),
    };
    let msg = strip_leading_level(strip_leading_timestamp(&rendered));
    format!("{lvl} {time} {msg}")
}

/// Read the merged, filtered log view: app + core tails, sorted by time, capped at `n`.
///
/// Display lines use a compact timestamp and omit source tags; the filter never
/// touches the log files themselves.
pub fn read_log_view(app_log: &Path, core_log: &Path, n: usize) -> Result<Vec<String>, AppError> {
    let n = n.min(VIEW_MAX);
    let mut lines: Vec<LogLine> = Vec::new();
    collect(&mut lines, Source::App, app_log)?;
    collect(&mut lines, Source::Core, core_log)?;
    lines.sort_by(|a, b| (a.ts, a.source, &a.text).cmp(&(b.ts, b.source, &b.text)));
    lines.truncate(n);
    Ok(lines
        .into_iter()
        .map(|l| format_display_line(l.ts, l.source, l.level, &l.text))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log_tail::strip_ansi;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "ice-box-logview-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn parses_app_format_line() {
        let line = "2026-08-23T13:47:01.123456Z  INFO ice_core: sing-box ready on 127.0.0.1:17890";
        let (ts, level) = parse_app_line(line).expect("parse");
        assert_eq!(level, Level::Info);
        assert_eq!(ts.to_rfc3339(), "2026-08-23T13:47:01.123456+00:00");
    }

    #[test]
    fn parses_core_format_line() {
        let line = "+0800 2026-08-23 13:47:01 INFO sing-box started (0.00s)";
        let (ts, level) = parse_core_line(line).expect("parse");
        assert_eq!(level, Level::Info);
        assert_eq!(ts.to_rfc3339(), "2026-08-23T13:47:01+08:00");
    }

    #[test]
    fn parses_core_line_with_ansi_colors() {
        let line = concat!(
            "+0800 2026-08-23 15:06:15 \u{1b}[36mINFO\u{1b}[0m \u{1b}[38;5;109m591640413\u{1b}[0m ",
            "outbound/trojan[\u{1b}[38;5;194m🇺🇸 美国实验性 IEPL 专线 1\u{1b}[0m]: ",
            "outbound connection to beacons.gcp.gvt2.com:443"
        );
        let (ts, level) = parse_core_line(&strip_ansi(line)).expect("parse");
        assert_eq!(level, Level::Info);
        assert_eq!(ts.to_rfc3339(), "2026-08-23T15:06:15+08:00");
        assert!(display_worthy(Source::Core, level, &strip_ansi(line)));
    }

    #[test]
    fn parses_core_line_without_zone() {
        let line = "2026-08-23 13:47:01 FATAL boom";
        let (_, level) = parse_core_line(line).expect("parse");
        assert_eq!(level, Level::Fatal);
    }

    #[test]
    fn rejects_unparseable_lines() {
        assert!(parse_app_line("garbage line").is_none());
        assert!(parse_core_line("short").is_none());
        assert!(parse_core_line("+0800 2026-08-23 13:47:01 unknown message").is_none());
    }

    #[test]
    fn filters_by_level_and_keywords() {
        assert!(display_worthy(Source::Core, Level::Warn, "anything"));
        assert!(display_worthy(Source::App, Level::Error, "anything"));
        assert!(display_worthy(
            Source::App,
            Level::Info,
            "file logging enabled"
        ));
        assert!(display_worthy(
            Source::Core,
            Level::Info,
            "sing-box started (0.00s)"
        ));
        assert!(display_worthy(
            Source::Core,
            Level::Info,
            "tcp server started at 127.0.0.1"
        ));
        assert!(display_worthy(
            Source::Core,
            Level::Info,
            "outbound/trojan[香港 IEPL 专线 1]: outbound connection to example.com:443"
        ));
        assert!(display_worthy(
            Source::Core,
            Level::Info,
            "[3591829452 0ms] outbound/selector[Proxies]: outbound connection to example.com:443"
        ));
        assert!(!display_worthy(
            Source::Core,
            Level::Info,
            "[TCP] dial example.com:443"
        ));
        assert!(!display_worthy(Source::App, Level::Debug, "noise"));
        assert!(!display_worthy(Source::Core, Level::Trace, "noise"));
    }

    #[test]
    fn parses_connection_route_tag_and_bare_type() {
        assert_eq!(
            parse_connection_route(
                "+0800 2026-08-23 15:06:15 INFO [591640413 0ms] outbound/trojan[🇺🇸 美国实验性 IEPL 专线 1]: outbound connection to beacons.gcp.gvt2.com:443"
            ),
            Some(("beacons.gcp.gvt2.com:443", "🇺🇸 美国实验性 IEPL 专线 1"))
        );
        assert_eq!(
            parse_connection_route(
                "INFO outbound/selector[Proxies]: outbound connection to example.com:443"
            ),
            Some(("example.com:443", "Proxies"))
        );
        assert_eq!(
            parse_connection_route(
                "INFO [3591829452 0ms] outbound/direct: outbound connection to 1.1.1.1:443"
            ),
            Some(("1.1.1.1:443", "direct"))
        );
        assert_eq!(
            parse_connection_route("INFO outbound/direct: dial tcp: connection refused"),
            None
        );
    }

    #[test]
    fn simplifies_core_lines_for_display() {
        assert_eq!(
            simplify_core_line("+0000 2026-08-23 13:47:01 INFO sing-box started (0.00s)"),
            "2026-08-23 13:47:01 INFO sing-box started (0.00s)"
        );
        assert_eq!(
            simplify_core_line(
                "+0000 2026-08-23 13:47:07 ERROR outbound/direct: dial tcp: connection refused"
            ),
            "2026-08-23 13:47:07 ERROR outbound/direct: dial tcp: connection refused"
        );
    }

    #[test]
    fn formats_compact_display_lines() {
        let app = "2026-08-23T13:47:01.123456Z  INFO ice_core: sing-box ready on 127.0.0.1:17890";
        let (ts, level) = parse_app_line(app).expect("parse");
        assert_eq!(
            format_display_line(ts, Source::App, level, app),
            "INFO 08-23 13:47:01 ice_core: sing-box ready on 127.0.0.1:17890"
        );

        let core = "+0800 2026-08-23 15:06:15 INFO [591640413 0ms] outbound/trojan[美国 1]: outbound connection to example.com:443";
        let (ts, level) = parse_core_line(core).expect("parse");
        assert_eq!(
            format_display_line(ts, Source::Core, level, core),
            "INFO 08-23 15:06:15 example.com:443 → 美国 1"
        );
    }

    #[test]
    fn merges_sorted_filtered_view() {
        let dir = temp_dir("merge");
        fs::create_dir_all(&dir).unwrap();
        let app = dir.join("ice-box.log");
        let core = dir.join("sing-box.log");

        fs::write(
            &app,
            concat!(
                "2026-08-23T13:47:02.000000Z  INFO ice_core: sing-box ready on 127.0.0.1:17890\n",
                "2026-08-23T13:47:03.000000Z  WARN ice_proxy_sys: proxy apply slow\n",
                "2026-08-23T13:47:04.000000Z DEBUG ice_core: probe loop tick\n",
            ),
        )
        .unwrap();
        fs::write(
            &core,
            concat!(
                "+0000 2026-08-23 13:47:01 INFO sing-box started (0.00s)\n",
                "+0000 2026-08-23 13:47:05 INFO [TCP] dial example.com:443\n",
                "+0000 2026-08-23 13:47:06 INFO [\u{1b}[38;5;109m591640413\u{1b}[0m 0ms] outbound/trojan[香港 IEPL 专线 1]: outbound connection to example.com:443\n",
                "+0000 2026-08-23 13:47:07 ERROR outbound/direct: dial tcp: connection refused\n",
            ),
        )
        .unwrap();

        let view = read_log_view(&app, &core, 500).unwrap();
        assert_eq!(
            view.len(),
            5,
            "debug hidden, outbound routing kept: {view:?}"
        );
        assert!(
            view.iter()
                .all(|l| !l.contains("[app]") && !l.contains("[core]")),
            "source tags stripped: {view:?}"
        );
        assert_eq!(view[0], "INFO 08-23 13:47:01 sing-box started (0.00s)");
        assert!(!view[0].contains("+0000"), "zone token stripped: {view:?}");
        assert_eq!(
            view[1],
            "INFO 08-23 13:47:02 ice_core: sing-box ready on 127.0.0.1:17890"
        );
        assert!(
            !view[1].contains('T') && !view[1].contains('Z'),
            "rfc3339 compacted: {view:?}"
        );
        assert!(view[2].contains("proxy apply slow"));
        assert!(view[2].starts_with("WARN 08-23 13:47:03 "));
        assert_eq!(
            view[3],
            "INFO 08-23 13:47:06 example.com:443 → 香港 IEPL 专线 1"
        );
        assert!(!view[3].contains('\u{1b}'), "ansi stripped from view");
        assert!(
            !view[3].contains("591640413"),
            "connection id stripped: {view:?}"
        );
        assert!(!view[3].contains("0ms]"), "delay prefix stripped: {view:?}");
        assert!(
            !view[3].contains("outbound/"),
            "protocol type dropped: {view:?}"
        );
        assert!(view[3].contains(" → "), "target/node separator: {view:?}");
        assert!(view[4].contains("connection refused"));
        assert!(view[4].starts_with("ERROR 08-23 13:47:07 "));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn caps_view_at_n() {
        let dir = temp_dir("cap");
        fs::create_dir_all(&dir).unwrap();
        let app = dir.join("ice-box.log");
        let core = dir.join("sing-box.log");
        let mut app_text = String::new();
        for i in 0..100 {
            app_text.push_str(&format!(
                "2026-08-23T13:47:{:02}.000000Z ERROR app error {i}\n",
                i % 60
            ));
        }
        fs::write(&app, app_text).unwrap();
        fs::write(&core, "").unwrap();

        let view = read_log_view(&app, &core, 3).unwrap();
        assert_eq!(view.len(), 3);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_files_yield_empty_view() {
        let dir = temp_dir("missing");
        let view = read_log_view(&dir.join("nope.log"), &dir.join("nope2.log"), 500).unwrap();
        assert!(view.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}
