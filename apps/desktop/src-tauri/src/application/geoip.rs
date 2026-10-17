// SPDX-License-Identifier: GPL-3.0-or-later

//! Country code for an IP, from the bundled sing-box `geoip-*.srs` rule-sets.
//!
//! The files are the routing sets the app already ships. This reader only
//! pulls `ip_cidr` ranges out of them so Home can name the landing node's
//! region without a network lookup.

use flate2::read::ZlibDecoder;
use std::fs;
use std::io::Read;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

type V4Cidr = (u32, u32);
type V6Cidr = (u128, u128);

struct GeoIndex {
    v4: Vec<(u32, u32, String)>,
    v6: Vec<(u128, u128, String)>,
}

/// Cached load for one directory. `index` is `None` when the directory is
/// missing, empty, or unreadable so a failed load is not retried on every probe.
struct CachedDir {
    dir: PathBuf,
    index: Option<GeoIndex>,
}

fn cache() -> &'static Mutex<Option<CachedDir>> {
    static CACHE: OnceLock<Mutex<Option<CachedDir>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// Lowercase country code (`hk`) when a bundled rule-set contains `ip`.
pub(crate) fn region_code(geoip_dir: &Path, ip: IpAddr) -> Option<String> {
    let guard = cached_index(geoip_dir);
    let index = guard.as_ref()?.index.as_ref()?;
    match ip {
        IpAddr::V4(addr) => find_v4(&index.v4, u32::from(addr)),
        IpAddr::V6(addr) => find_v6(&index.v6, u128::from(addr)),
    }
}

fn cached_index(geoip_dir: &Path) -> std::sync::MutexGuard<'static, Option<CachedDir>> {
    let mut slot = cache().lock().unwrap_or_else(|err| err.into_inner());
    if slot.as_ref().is_some_and(|cached| cached.dir == geoip_dir) {
        return slot;
    }
    let index = load_index(geoip_dir);
    *slot = Some(CachedDir {
        dir: geoip_dir.to_path_buf(),
        index,
    });
    slot
}

/// Ranges from different country files can overlap. After the binary search
/// lands on the rightmost `start <= ip`, walk left through every such start and
/// keep the tightest containing range (then the lexicographically smallest
/// country code) so a wider neighbour cannot hide a nested match.
fn find_v4(ranges: &[(u32, u32, String)], ip: u32) -> Option<String> {
    find_overlapping(ranges, ip, |start, end, addr| {
        *start <= addr && addr <= *end
    })
}

fn find_v6(ranges: &[(u128, u128, String)], ip: u128) -> Option<String> {
    find_overlapping(ranges, ip, |start, end, addr| {
        *start <= addr && addr <= *end
    })
}

fn find_overlapping<A>(
    ranges: &[(A, A, String)],
    ip: A,
    contains: impl Fn(&A, &A, A) -> bool,
) -> Option<String>
where
    A: Copy + Ord + std::ops::Sub<Output = A>,
{
    let mut lo = 0usize;
    let mut hi = ranges.len();
    while lo < hi {
        let mid = (lo + hi) / 2;
        if ranges[mid].0 <= ip {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    if lo == 0 {
        return None;
    }
    let mut best: Option<(A, &str)> = None;
    for idx in (0..lo).rev() {
        let (start, end, code) = &ranges[idx];
        if *start > ip {
            continue;
        }
        if !contains(start, end, ip) {
            continue;
        }
        let span = *end - *start;
        match best {
            None => best = Some((span, code.as_str())),
            Some((best_span, best_code)) => {
                if span < best_span || (span == best_span && code.as_str() < best_code) {
                    best = Some((span, code.as_str()));
                }
            }
        }
    }
    best.map(|(_, code)| code.to_string())
}

fn load_index(dir: &Path) -> Option<GeoIndex> {
    let entries = fs::read_dir(dir).ok()?;
    let mut v4 = Vec::new();
    let mut v6 = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(code) = name
            .strip_prefix("geoip-")
            .and_then(|rest| rest.strip_suffix(".srs"))
        else {
            continue;
        };
        if code.len() != 2 || !code.bytes().all(|byte| byte.is_ascii_lowercase()) {
            continue;
        }
        let Ok(bytes) = fs::read(entry.path()) else {
            continue;
        };
        if let Some((file_v4, file_v6)) = parse_srs(&bytes) {
            for (start, end) in file_v4 {
                v4.push((start, end, code.to_string()));
            }
            for (start, end) in file_v6 {
                v6.push((start, end, code.to_string()));
            }
        }
    }
    if v4.is_empty() && v6.is_empty() {
        return None;
    }
    // Sort by start, then end ascending so nested ranges stay after their
    // outer neighbours with the same start and the overlap walk is stable.
    v4.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    v6.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    Some(GeoIndex { v4, v6 })
}

/// `None` when the file is not a geoip rule-set we know how to read.
fn parse_srs(bytes: &[u8]) -> Option<(Vec<V4Cidr>, Vec<V6Cidr>)> {
    if bytes.len() < 5 || &bytes[..3] != b"SRS" {
        return None;
    }
    let mut decoder = ZlibDecoder::new(&bytes[4..]);
    let mut data = Vec::new();
    decoder.read_to_end(&mut data).ok()?;
    let mut index = 0usize;
    let (rules, next) = read_uvarint(&data, index)?;
    index = next;
    let mut v4 = Vec::new();
    let mut v6 = Vec::new();
    for _ in 0..rules {
        let rule_type = *data.get(index)?;
        index += 1;
        if rule_type != 0 {
            return None;
        }
        loop {
            let item = *data.get(index)?;
            index += 1;
            if item == 0xFF {
                index += 1;
                break;
            }
            if item != 6 {
                return None;
            }
            let version = *data.get(index)?;
            index += 1;
            if version != 1 || index + 8 > data.len() {
                return None;
            }
            let count = u64::from_be_bytes(data[index..index + 8].try_into().ok()?);
            index += 8;
            for _ in 0..count {
                let (start, next) = read_addr(&data, index)?;
                index = next;
                let (end, next) = read_addr(&data, index)?;
                index = next;
                match (start, end) {
                    (Addr::V4(start), Addr::V4(end)) => v4.push((start, end)),
                    (Addr::V6(start), Addr::V6(end)) => v6.push((start, end)),
                    _ => return None,
                }
            }
        }
    }
    Some((v4, v6))
}

enum Addr {
    V4(u32),
    V6(u128),
}

fn read_addr(data: &[u8], index: usize) -> Option<(Addr, usize)> {
    let (len, index) = read_uvarint(data, index)?;
    let len = usize::try_from(len).ok()?;
    if index + len > data.len() {
        return None;
    }
    let bytes = &data[index..index + len];
    let addr = match len {
        4 => Addr::V4(u32::from_be_bytes(bytes.try_into().ok()?)),
        16 => Addr::V6(u128::from_be_bytes(bytes.try_into().ok()?)),
        _ => return None,
    };
    Some((addr, index + len))
}

fn read_uvarint(data: &[u8], mut index: usize) -> Option<(u64, usize)> {
    let mut value = 0u64;
    let mut shift = 0;
    loop {
        if index >= data.len() || shift > 63 {
            return None;
        }
        let byte = data[index];
        index += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte < 0x80 {
            return Some((value, index));
        }
        shift += 7;
    }
}

#[cfg(test)]
mod tests {
    use super::{find_v4, load_index, region_code};
    use std::net::IpAddr;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn bundled_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../third_party/sing-geoip/rule-set")
    }

    #[test]
    fn matches_a_hong_kong_range_and_not_a_public_resolver() {
        let dir = bundled_dir();
        let hk: IpAddr = "1.36.1.1".parse().unwrap();
        let resolver: IpAddr = "8.8.8.8".parse().unwrap();
        assert_eq!(region_code(&dir, hk).as_deref(), Some("hk"));
        assert_ne!(region_code(&dir, resolver).as_deref(), Some("hk"));
        assert_eq!(region_code(&dir, resolver).as_deref(), Some("us"));
    }

    #[test]
    fn prefers_the_tightest_overlapping_range() {
        // A wide `zz` range surrounds a nested `aa` block. Nearest-preceding
        // alone would keep `zz` when `aa` starts later; the overlap walk must
        // return the nested country.
        let ranges = vec![
            (10u32, 100u32, "zz".to_string()),
            (40u32, 50u32, "aa".to_string()),
        ];
        assert_eq!(find_v4(&ranges, 45).as_deref(), Some("aa"));
        assert_eq!(find_v4(&ranges, 20).as_deref(), Some("zz"));
        assert_eq!(find_v4(&ranges, 200), None);
    }

    #[test]
    fn a_missing_geoip_dir_is_cached_as_empty() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let missing = std::env::temp_dir().join(format!("ice-box-geoip-missing-{stamp}"));
        assert!(!missing.exists());
        assert!(load_index(&missing).is_none());
        let missing_ip: IpAddr = "1.2.3.4".parse().unwrap();
        assert_eq!(region_code(&missing, missing_ip), None);
        // A second lookup must still miss without panicking; the cache holds
        // the failed load for this path.
        assert_eq!(region_code(&missing, missing_ip), None);
    }
}
