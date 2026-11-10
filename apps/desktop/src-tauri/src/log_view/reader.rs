// SPDX-License-Identifier: GPL-3.0-or-later

//! One-shot tail read of a single log source.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use super::{
    display_worthy, format_display_line, parse_app_line, parse_core_line, Source, SCAN_PER_SOURCE,
};
use crate::log_tail::strip_ansi;
use chrono::{DateTime, FixedOffset};

/// Bytes read from the end of each source.
const SCAN_BYTES: u64 = 4 * 1024 * 1024;
/// Longer lines are cut to this many bytes and marked.
const LINE_BYTES: usize = 64 * 1024;

pub(super) struct Line {
    pub(super) ts: DateTime<FixedOffset>,
    pub(super) text: String,
}

/// Display lines from the last `SCAN_PER_SOURCE` lines (at most `SCAN_BYTES`)
/// of `path`, in file order. `all` keeps lines the default view hides. A
/// missing file has no lines.
pub(super) fn read_tail(path: &Path, source: Source, all: bool) -> io::Result<Vec<Line>> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };
    let len = file.metadata()?.len();
    // Start one byte early: when the tail starts mid-file, that byte tells
    // whether the first line read is whole or a fragment to drop.
    let start = len.saturating_sub(SCAN_BYTES + 1);
    file.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::new();
    file.take(len - start).read_to_end(&mut buf)?;
    let body = if start == 0 {
        &buf[..]
    } else {
        match buf.iter().position(|&byte| byte == b'\n') {
            Some(newline) => &buf[newline + 1..],
            None => &[][..],
        }
    };
    let mut raws: Vec<&[u8]> = body.split(|&byte| byte == b'\n').collect();
    // A file that ends with a newline leaves an empty last piece; an
    // unterminated last line is kept.
    if raws.last().is_some_and(|raw| raw.is_empty()) {
        raws.pop();
    }
    let skip = raws.len().saturating_sub(SCAN_PER_SOURCE);
    Ok(raws[skip..]
        .iter()
        .filter_map(|raw| parse_line(raw, source, all))
        .collect())
}

fn parse_line(raw: &[u8], source: Source, all: bool) -> Option<Line> {
    let cut = raw.len() > LINE_BYTES;
    let raw = strip_ansi(&String::from_utf8_lossy(&raw[..raw.len().min(LINE_BYTES)]));
    let (ts, level) = match source {
        Source::App => parse_app_line(&raw),
        Source::Core => parse_core_line(&raw),
    }?;
    if !all && !display_worthy(source, level, &raw) {
        return None;
    }
    let mut text = format_display_line(ts, source, level, &raw);
    if cut {
        text.push_str(" … [truncated]");
    }
    Some(Line { ts, text })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn fixture(label: &str) -> (PathBuf, PathBuf) {
        let dir = super::super::tests::temp_dir(label);
        fs::create_dir_all(&dir).unwrap();
        (dir.join("app.log"), dir)
    }

    fn texts(lines: Vec<Line>) -> Vec<String> {
        lines.into_iter().map(|line| line.text).collect()
    }

    #[test]
    fn missing_file_has_no_lines() {
        let (app, dir) = fixture("reader-missing");
        assert!(read_tail(&app, Source::App, true).unwrap().is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn keeps_an_unterminated_last_line_and_utf8() {
        let (app, dir) = fixture("reader-partial");
        fs::write(
            &app,
            "2026-08-23T13:47:01Z DEBUG shown\n2026-08-23T13:47:02Z INFO 中文",
        )
        .unwrap();
        let view = texts(read_tail(&app, Source::App, true).unwrap());
        assert_eq!(view.len(), 2);
        assert!(view[0].ends_with("shown"));
        assert!(view[1].ends_with("中文"));
        let filtered = texts(read_tail(&app, Source::App, false).unwrap());
        assert_eq!(filtered.len(), 1, "DEBUG hidden by default: {filtered:?}");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn cuts_long_lines_and_marks_them() {
        let (app, dir) = fixture("reader-long");
        let long = format!("2026-08-23T13:47:01Z INFO {}\n", "x".repeat(LINE_BYTES * 2));
        fs::write(&app, long).unwrap();
        let view = texts(read_tail(&app, Source::App, true).unwrap());
        assert_eq!(view.len(), 1);
        assert!(view[0].ends_with(" … [truncated]"));
        assert!(view[0].len() <= LINE_BYTES + 64);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reads_only_the_last_lines_of_a_long_file() {
        let (app, dir) = fixture("reader-scan");
        let mut text = "2026-08-23T13:47:01Z INFO early\n".to_string();
        text.push_str(&"ignored\n".repeat(SCAN_PER_SOURCE));
        fs::write(&app, &text).unwrap();
        assert!(
            read_tail(&app, Source::App, true).unwrap().is_empty(),
            "a line before the last SCAN_PER_SOURCE is not read"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_tail_that_starts_mid_line_drops_the_fragment() {
        let (app, dir) = fixture("reader-fragment");
        let scan = SCAN_BYTES as usize;
        // The scanned bytes start exactly at a timestamp inside the first
        // line, so the fragment would parse as a line if it were kept.
        let whole = "2026-08-23T13:47:02Z INFO whole\n";
        let head = "2026-08-23T13:47:01Z INFO fragment";
        let fragment = format!(
            "{head}{}\n",
            "f".repeat(scan - whole.len() - head.len() - 1)
        );
        fs::write(
            &app,
            format!("2026-08-23T13:47:00Z INFO head {fragment}{whole}"),
        )
        .unwrap();
        let view = texts(read_tail(&app, Source::App, true).unwrap());
        assert_eq!(view.len(), 1, "{view:?}");
        assert!(view[0].ends_with("whole"));

        // A tail that starts right after a newline keeps its first line.
        let head = "2026-08-23T13:47:03Z INFO first";
        let first = format!("{head}{}\n", "f".repeat(scan - head.len() - 1));
        fs::write(&app, format!("yyyy\n{first}")).unwrap();
        let view = texts(read_tail(&app, Source::App, true).unwrap());
        assert_eq!(view.len(), 1, "{view:?}");
        assert!(view[0].contains("first"));
        fs::remove_dir_all(dir).unwrap();
    }
}
