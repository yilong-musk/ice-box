// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::VecDeque;
use std::fs::{File, Metadata};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::{
    display_worthy, format_display_line, parse_app_line, parse_core_line, Source, SCAN_PER_SOURCE,
    VIEW_MAX,
};
use crate::log_tail::strip_ansi;
use chrono::{DateTime, FixedOffset};
use ice_config::{AppError, ErrorCode};

const SCAN_BYTES: u64 = 4 * 1024 * 1024;
const RETAINED_BYTES: usize = 1024 * 1024;
const LINE_BYTES: usize = 64 * 1024;
const ANCHOR_BYTES: usize = 128;

#[derive(PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
    created: Option<SystemTime>,
    #[cfg(unix)]
    identity: (u64, u64),
}

impl Stamp {
    fn new(meta: &Metadata) -> Self {
        Self {
            len: meta.len(),
            modified: meta.modified().ok(),
            created: meta.created().ok(),
            #[cfg(unix)]
            identity: {
                use std::os::unix::fs::MetadataExt;
                (meta.dev(), meta.ino())
            },
        }
    }

    fn same_file(&self, other: &Self) -> bool {
        #[cfg(unix)]
        if self.identity != other.identity {
            return false;
        }
        self.created == other.created
    }
}

struct Line {
    ts: DateTime<FixedOffset>,
    order: u64,
    text: String,
}

#[derive(Default)]
struct SourceReader {
    path: PathBuf,
    stamp: Option<Stamp>,
    offset: u64,
    anchor: Vec<u8>,
    pending: Vec<u8>,
    skip_fragment: bool,
    oversized: bool,
    order: u64,
    lines: VecDeque<Line>,
    bytes: usize,
    preview: Option<Line>,
    #[cfg(test)]
    parsed: usize,
}

impl SourceReader {
    fn parse(&mut self, raw: &[u8], source: Source, debug: bool) -> Option<Line> {
        #[cfg(test)]
        {
            self.parsed += 1;
        }
        let raw = strip_ansi(&String::from_utf8_lossy(raw));
        let (ts, level) = match source {
            Source::App => parse_app_line(&raw),
            Source::Core => parse_core_line(&raw),
        }?;
        (debug || display_worthy(source, level, &raw)).then(|| Line {
            ts,
            order: self.order,
            text: format_display_line(ts, source, level, &raw),
        })
    }

    fn finish_line(&mut self, source: Source, debug: bool) {
        if !self.skip_fragment {
            let raw = std::mem::take(&mut self.pending);
            if let Some(mut line) = self.parse(&raw, source, debug) {
                if self.oversized {
                    line.text.push_str(" … [truncated]");
                }
                self.bytes += line.text.len();
                self.lines.push_back(line);
            }
            self.pending = raw;
        }
        self.pending.clear();
        self.skip_fragment = false;
        self.oversized = false;
        self.order += 1;
        while self.lines.front().is_some_and(|line| {
            self.order - line.order > SCAN_PER_SOURCE as u64
                || self.lines.len() > VIEW_MAX
                || self.bytes > RETAINED_BYTES
        }) {
            self.bytes -= self.lines.pop_front().unwrap().text.len();
        }
    }

    fn refresh(&mut self, path: &Path, source: Source, debug: bool) -> io::Result<bool> {
        let meta = match std::fs::metadata(path) {
            Ok(meta) => meta,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                let changed = self.stamp.is_some();
                *self = Self::default();
                return Ok(changed);
            }
            Err(err) => return Err(err),
        };
        let stamp = Stamp::new(&meta);
        if self.path == path && self.stamp.as_ref() == Some(&stamp) {
            return Ok(false);
        }
        let mut file = File::open(path)?;
        // Recheck the opened handle: the path may have rotated after metadata().
        let stamp = Stamp::new(&file.metadata()?);
        let mut reset = self.path != path
            || self.stamp.as_ref().is_none_or(|old| !old.same_file(&stamp))
            || stamp.len < self.offset
            || stamp.len - self.offset > SCAN_BYTES;
        if !reset && !self.anchor.is_empty() {
            file.seek(SeekFrom::Start(self.offset - self.anchor.len() as u64))?;
            let mut anchor = vec![0; self.anchor.len()];
            reset = file.read_exact(&mut anchor).is_err() || anchor != self.anchor;
            // A same-length modification is a rewrite, not an append.
            reset |= stamp.len == self.offset;
        }
        if reset {
            *self = Self::default();
            self.path = path.to_owned();
            self.offset = stamp.len.saturating_sub(SCAN_BYTES);
            if self.offset > 0 {
                file.seek(SeekFrom::Start(self.offset - 1))?;
                let mut previous = [0];
                file.read_exact(&mut previous)?;
                self.skip_fragment = previous[0] != b'\n';
            }
        }
        file.seek(SeekFrom::Start(self.offset))?;
        let mut remaining = stamp.len - self.offset;
        let mut buf = [0u8; 16 * 1024];
        while remaining > 0 {
            let want = remaining.min(buf.len() as u64) as usize;
            let count = file.read(&mut buf[..want])?;
            if count == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            for &byte in &buf[..count] {
                if byte == b'\n' {
                    self.finish_line(source, debug);
                } else if !self.skip_fragment {
                    if self.pending.len() < LINE_BYTES {
                        self.pending.push(byte);
                    } else {
                        self.oversized = true;
                    }
                }
            }
            self.offset += count as u64;
            remaining -= count as u64;
        }
        // Preserve the old view's unterminated final line without inserting it
        // twice when the next append finishes it (including split UTF-8).
        self.preview = None;
        if !self.pending.is_empty() && !self.skip_fragment {
            let raw = std::mem::take(&mut self.pending);
            self.preview = self.parse(&raw, source, debug);
            self.pending = raw;
        }
        let anchor_len = self.offset.min(ANCHOR_BYTES as u64) as usize;
        self.anchor.resize(anchor_len, 0);
        file.seek(SeekFrom::Start(self.offset - anchor_len as u64))?;
        file.read_exact(&mut self.anchor)?;
        self.stamp = Some(stamp);
        Ok(true)
    }
}

/// Polling retains bounded parsed tails and reads only newly appended bytes.
/// Rotation, truncation/regrowth, debug changes and source changes reset safely.
#[derive(Default)]
pub(crate) struct LogViewReader {
    sources: [SourceReader; 3],
    debug: Option<bool>,
    n: usize,
    rendered: Vec<String>,
}

impl LogViewReader {
    pub(crate) fn read(
        &mut self,
        app: &Path,
        core: &Path,
        helper: Option<&Path>,
        n: usize,
        debug: bool,
    ) -> Result<Vec<String>, AppError> {
        let n = n.min(VIEW_MAX);
        let mut changed = self.n != n || self.debug != Some(debug);
        if self.debug != Some(debug) {
            self.sources = Default::default();
            self.debug = Some(debug);
        }
        for (i, (path, source)) in [
            (Some(app), Source::App),
            (Some(core), Source::Core),
            (helper, Source::Core),
        ]
        .into_iter()
        .enumerate()
        {
            let Some(path) = path else {
                changed |= self.sources[i].stamp.is_some();
                self.sources[i] = SourceReader::default();
                continue;
            };
            match self.sources[i].refresh(path, source, debug) {
                Ok(dirty) => changed |= dirty,
                Err(err) => {
                    self.sources[i] = SourceReader::default();
                    changed = true;
                    if i != 2 {
                        // Force a rebuild after the next successful retry.
                        self.debug = None;
                        return Err(AppError::new(
                            ErrorCode::ConfigInvalid,
                            format!("read log {}: {err}", path.display()),
                        ));
                    }
                }
            }
        }
        if changed {
            let mut lines = self
                .sources
                .iter()
                .enumerate()
                .flat_map(|(source, reader)| {
                    reader
                        .lines
                        .iter()
                        .chain(reader.preview.iter())
                        .map(move |line| (source, line))
                })
                .collect::<Vec<_>>();
            lines.sort_by_key(|(source, line)| (line.ts, *source, line.order));
            self.rendered = lines
                .iter()
                .skip(lines.len().saturating_sub(n))
                .map(|(_, line)| line.text.clone())
                .collect();
            self.n = n;
        }
        Ok(self.rendered.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, OpenOptions};
    use std::io::Write;

    fn fixture(label: &str) -> (PathBuf, PathBuf, PathBuf) {
        let dir = super::super::tests::temp_dir(label);
        fs::create_dir_all(&dir).unwrap();
        (dir.join("app.log"), dir.join("core.log"), dir)
    }

    fn append(path: &Path, bytes: &[u8]) {
        OpenOptions::new()
            .append(true)
            .open(path)
            .unwrap()
            .write_all(bytes)
            .unwrap();
    }

    #[test]
    fn unchanged_history_is_not_reparsed_and_append_is_incremental() {
        let (app, core, dir) = fixture("incremental");
        fs::write(&app, "2026-08-23T13:47:01Z INFO one\n").unwrap();
        let mut reader = LogViewReader::default();
        let first = reader.read(&app, &core, None, 500, false).unwrap();
        assert_eq!(reader.sources[0].parsed, 1);
        assert_eq!(reader.read(&app, &core, None, 500, false).unwrap(), first);
        assert_eq!(reader.sources[0].parsed, 1);
        append(&app, b"2026-08-23T13:47:02Z INFO two\n");
        let next = reader.read(&app, &core, None, 500, false).unwrap();
        assert_eq!(next.len(), 2);
        assert_eq!(reader.sources[0].parsed, 2);
        assert_eq!(reader.read(&app, &core, None, 1, false).unwrap(), next[1..]);
        assert_eq!(reader.sources[0].parsed, 2);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn partial_utf8_lines_are_completed_once_and_debug_resets_filtering() {
        let (app, core, dir) = fixture("partial");
        fs::write(
            &app,
            "2026-08-23T13:47:01Z DEBUG hidden\n2026-08-23T13:47:02Z INFO ",
        )
        .unwrap();
        let mut reader = LogViewReader::default();
        append(&app, &"中文".as_bytes()[..2]);
        reader.read(&app, &core, None, 500, false).unwrap();
        append(&app, &"中文".as_bytes()[2..]);
        append(&app, b"\n");
        let view = reader.read(&app, &core, None, 500, false).unwrap();
        assert_eq!(view.len(), 1);
        assert!(view[0].ends_with("中文"));
        assert_eq!(reader.read(&app, &core, None, 500, true).unwrap().len(), 2);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn truncation_regrowth_rotation_and_removal_discard_stale_lines() {
        let (app, core, dir) = fixture("rotation");
        fs::write(&app, "2026-08-23T13:47:01Z INFO old\n").unwrap();
        let mut reader = LogViewReader::default();
        reader.read(&app, &core, None, 500, false).unwrap();
        fs::write(&app, "2026-08-23T13:47:02Z INFO new and longer\n").unwrap();
        let next = reader.read(&app, &core, None, 500, false).unwrap();
        assert_eq!(next.len(), 1);
        assert!(next[0].ends_with("new and longer"));
        fs::rename(&app, dir.join("rotated.log")).unwrap();
        fs::write(&app, "2026-08-23T13:47:03Z INFO rotated\n").unwrap();
        let next = reader.read(&app, &core, None, 500, false).unwrap();
        assert_eq!(next.len(), 1);
        assert!(next[0].ends_with("rotated"));
        fs::write(&app, b"").unwrap();
        assert!(reader
            .read(&app, &core, None, 500, false)
            .unwrap()
            .is_empty());
        fs::remove_file(&app).unwrap();
        assert!(reader
            .read(&app, &core, None, 500, false)
            .unwrap()
            .is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn retained_rows_and_unterminated_lines_have_hard_memory_bounds() {
        let (app, core, dir) = fixture("bounds");
        let line = "2026-08-23T13:47:01Z INFO entry\n";
        fs::write(&app, line.repeat(2000)).unwrap();
        let mut reader = LogViewReader::default();
        assert_eq!(
            reader
                .read(&app, &core, None, usize::MAX, false)
                .unwrap()
                .len(),
            VIEW_MAX
        );
        assert_eq!(reader.sources[0].lines.len(), VIEW_MAX);
        append(&app, &vec![b'x'; LINE_BYTES * 3]);
        reader.read(&app, &core, None, 500, false).unwrap();
        assert!(reader.sources[0].pending.len() <= LINE_BYTES);
        assert!(reader.sources[0].bytes <= RETAINED_BYTES);
        append(&app, &"\nignored\n".repeat(SCAN_PER_SOURCE).into_bytes());
        assert!(reader
            .read(&app, &core, None, 500, false)
            .unwrap()
            .is_empty());
        fs::remove_dir_all(dir).unwrap();
    }
}
