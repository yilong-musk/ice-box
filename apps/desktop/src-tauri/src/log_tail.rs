//! Efficient log file tail (architecture §16: n ≤ 500, avoid full-file read).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use ice_config::{AppError, ErrorCode};

pub const LOG_TAIL_MAX: usize = 500;
/// Hard cap for scan reads: the merged view reads deeper before filtering, but must
/// stay bounded to avoid pulling whole files into memory (architecture §16).
pub const LOG_SCAN_MAX: usize = 10_000;

const INITIAL_WINDOW: u64 = 256 * 1024;
const MAX_WINDOW: u64 = 4 * 1024 * 1024;

/// Strip ANSI SGR escape sequences (`ESC[...m`, e.g. sing-box color codes) so log
/// parsers and the display see clean text. Raw log files are never modified.
pub fn strip_ansi(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b {
            // CSI: ESC [ ... final byte in 0x40..=0x7E
            let mut j = i + 1;
            if j < bytes.len() && bytes[j] == b'[' {
                j += 1;
                while j < bytes.len() && !(0x40..=0x7e).contains(&bytes[j]) {
                    j += 1;
                }
                if j < bytes.len() {
                    j += 1;
                }
            } else {
                j = i + 1;
            }
            i = j;
        } else {
            let ch_len = utf8_len(bytes[i]);
            out.push_str(&s[i..i + ch_len]);
            i += ch_len;
        }
    }
    out
}

fn utf8_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b >> 5 == 0b110 {
        2
    } else if b >> 4 == 0b1110 {
        3
    } else if b >> 3 == 0b11110 {
        4
    } else {
        1
    }
}

fn tail_lines_from_window(text: &str, from_start: bool, n: usize) -> Vec<String> {
    let mut lines: Vec<&str> = text.lines().collect();
    if !from_start && !lines.is_empty() {
        lines.remove(0);
    }
    let skip = lines.len().saturating_sub(n);
    lines[skip..].iter().map(|s| (*s).to_string()).collect()
}

/// Read up to `n` trailing lines (capped at [`LOG_TAIL_MAX`]) by scanning backward.
/// Kept for tests (acceptance G9.8) and any caller that wants the raw, unfiltered tail.
#[cfg_attr(not(test), allow(dead_code))]
pub fn read_log_tail(path: &Path, n: usize) -> Result<Vec<String>, AppError> {
    read_tail(path, n.min(LOG_TAIL_MAX))
}

/// Read up to `n` trailing lines (capped at [`LOG_SCAN_MAX`]); used by the merged view
/// so filtering still yields enough lines. Callers decide the policy; raw log files are
/// never modified here.
pub fn read_log_tail_deep(path: &Path, n: usize) -> Result<Vec<String>, AppError> {
    read_tail(path, n.min(LOG_SCAN_MAX))
}

fn read_tail(path: &Path, n: usize) -> Result<Vec<String>, AppError> {
    if n == 0 {
        return Ok(Vec::new());
    }
    if !path.exists() {
        return Ok(Vec::new());
    }

    let mut file = File::open(path).map_err(|e| {
        AppError::new(
            ErrorCode::ConfigInvalid,
            format!("open log {}: {e}", path.display()),
        )
    })?;
    let len = file
        .seek(SeekFrom::End(0))
        .map_err(|e| AppError::new(ErrorCode::ConfigInvalid, format!("seek log: {e}")))?;
    if len == 0 {
        return Ok(Vec::new());
    }

    let mut window = INITIAL_WINDOW.min(len);
    loop {
        let start = len.saturating_sub(window);
        file.seek(SeekFrom::Start(start))
            .map_err(|e| AppError::new(ErrorCode::ConfigInvalid, format!("seek log start: {e}")))?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)
            .map_err(|e| AppError::new(ErrorCode::ConfigInvalid, format!("read log: {e}")))?;

        let text = String::from_utf8_lossy(&buf);
        let text = strip_ansi(&text);
        let lines = tail_lines_from_window(&text, start == 0, n);
        if lines.len() >= n || start == 0 {
            return Ok(lines);
        }

        if window >= len {
            return Ok(lines);
        }

        let next = window.saturating_mul(2).min(len).min(MAX_WINDOW);
        if next <= window {
            return Ok(lines);
        }
        window = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn g7_10_caps_at_500_and_handles_501_request() {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-logtail-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ice-box.log");
        let mut f = fs::File::create(&path).unwrap();
        for i in 0..600 {
            writeln!(f, "line-{i}").unwrap();
        }
        drop(f);

        let lines = read_log_tail(&path, 500).unwrap();
        assert_eq!(lines.len(), 500);
        assert_eq!(lines[0], "line-100");
        assert_eq!(lines[499], "line-599");

        let lines501 = read_log_tail(&path, 501).unwrap();
        assert_eq!(lines501.len(), 500);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn strips_ansi_escapes() {
        let raw = "\x1b[36mINFO\x1b[0m [\x1b[38;5;194m123\x1b[0m] outbound/trojan[🇭🇰 香港 1]: outbound connection to example.com:443";
        let clean = strip_ansi(raw);
        assert_eq!(
            clean,
            "INFO [123] outbound/trojan[🇭🇰 香港 1]: outbound connection to example.com:443"
        );
        assert_eq!(strip_ansi("plain text"), "plain text");
        assert_eq!(strip_ansi(""), "");
    }

    #[test]
    fn tail_strips_ansi_from_real_core_lines() {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-logtail-ansi-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sing-box.log");
        fs::write(
            &path,
            "+0800 2026-08-23 15:06:15 \x1b[36mINFO\x1b[0m \x1b[38;5;109m591640413\x1b[0m outbound/trojan[🇺🇸 美国实验性 IEPL 专线 1]: outbound connection to beacons.gcp.gvt2.com:443\n",
        )
        .unwrap();

        let lines = read_log_tail(&path, 500).unwrap();
        assert_eq!(lines.len(), 1);
        assert!(!lines[0].contains('\u{1b}'));
        assert!(lines[0].contains("INFO"));
        assert!(lines[0].contains("outbound connection to"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn expands_window_for_very_long_lines() {
        let dir = std::env::temp_dir().join(format!(
            "ice-box-logtail-long-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ice-box.log");
        let mut f = fs::File::create(&path).unwrap();
        for i in 0..120 {
            writeln!(f, "line-{i}-{}", "x".repeat(4096)).unwrap();
        }
        drop(f);

        let lines = read_log_tail(&path, 100).unwrap();
        assert_eq!(lines.len(), 100);
        assert!(lines[0].starts_with("line-20-"));
        assert!(lines[99].starts_with("line-119-"));

        let _ = fs::remove_dir_all(&dir);
    }
}
