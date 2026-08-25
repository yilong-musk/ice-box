//! Named WinInet connections: RAS/VPN phonebook plus leftover Connections-key names.

use std::collections::BTreeSet;
use std::mem::{size_of, zeroed};

use windows_sys::Win32::NetworkManagement::Rras::{
    RasEnumEntriesW, ERROR_BUFFER_TOO_SMALL, RASENTRYNAMEW,
};
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

use super::wide::wide_buf_to_string;

const CONNECTIONS_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Internet Settings\Connections";

const SKIP_CONNECTION_VALUES: &[&str] = &[
    "DefaultConnectionSettings",
    "SavedLegacySettings",
    "WinHttpSettings",
];

fn skip_registry_name(name: &str) -> bool {
    SKIP_CONNECTION_VALUES
        .iter()
        .any(|skip| skip.eq_ignore_ascii_case(name))
}

/// RAS / VPN entries from the phone book. Enumeration failure yields an empty
/// list (LAN still applies); ice-box then will not mutate RAS either.
pub fn ras_connection_names() -> Vec<String> {
    let mut cb = 0u32;
    let mut count = 0u32;
    // SAFETY: first call with a null array is the documented size probe.
    let first = unsafe {
        RasEnumEntriesW(
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            &mut cb,
            &mut count,
        )
    };
    if first == 0 {
        return Vec::new();
    }
    if first != ERROR_BUFFER_TOO_SMALL {
        tracing::warn!(
            code = first,
            "RasEnumEntries failed; applying the LAN connection only"
        );
        return Vec::new();
    }
    if cb == 0 {
        return Vec::new();
    }

    let entry_size = size_of::<RASENTRYNAMEW>();
    let n = (cb as usize).div_ceil(entry_size).max(1);
    let mut entries: Vec<RASENTRYNAMEW> = (0..n)
        .map(|_| {
            // SAFETY: RASENTRYNAMEW is a POD struct of integers / UTF-16 arrays.
            unsafe { zeroed() }
        })
        .collect();
    entries[0].dwSize = entry_size as u32;
    cb = (n * entry_size) as u32;
    count = 0;
    // SAFETY: `entries` is a correctly sized RASENTRYNAMEW array; dwSize is set.
    let rc = unsafe {
        RasEnumEntriesW(
            std::ptr::null(),
            std::ptr::null(),
            entries.as_mut_ptr(),
            &mut cb,
            &mut count,
        )
    };
    if rc != 0 {
        tracing::warn!(
            code = rc,
            "RasEnumEntries(buffer) failed; applying the LAN connection only"
        );
        return Vec::new();
    }

    entries
        .iter()
        .take(count as usize)
        .filter_map(|entry| wide_buf_to_string(&entry.szEntryName))
        .collect()
}

fn registry_connection_names() -> Vec<String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok(key) = hkcu.open_subkey(CONNECTIONS_KEY) else {
        return Vec::new();
    };
    key.enum_values()
        .filter_map(|item| item.ok().map(|(name, _)| name))
        .filter(|name| !skip_registry_name(name))
        .collect()
}

/// Named connections only (LAN is handled separately as `None`).
pub fn named_connection_names() -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut names = Vec::new();
    for name in ras_connection_names()
        .into_iter()
        .chain(registry_connection_names())
    {
        if seen.insert(name.clone()) {
            names.push(name);
        }
    }
    names
}
