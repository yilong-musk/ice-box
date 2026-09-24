// SPDX-License-Identifier: GPL-3.0-or-later

//! Resident set size (RSS) of one process, for the Home memory readout.
//!
//! Scope (plan v0.1.14 §9, D5): the core process (sing-box) and the app's own
//! main process only. WebView helpers (WebKit / WebView2) and the privileged
//! helper daemon are out of scope, so the figure is lower than the "whole app"
//! number in Activity Monitor / Task Manager.
//!
//! macOS degradation (D9a): a privileged core (helper / TUN) runs as root and
//! its memory cannot be read from the unprivileged app (`proc_pidinfo` fails
//! with EPERM). That surfaces as an error here and the caller falls back to the
//! app-only figure — never elevated just to read it.

use std::io;

/// Resident memory of `pid` in bytes.
///
/// Errors are expected (process gone, no permission): callers treat them as
/// "unreadable" and degrade the readout instead of inventing a number.
#[cfg(target_os = "macos")]
pub fn resident_bytes(pid: u32) -> io::Result<u64> {
    let mut info: libc::proc_taskinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
    let written = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDTASKINFO,
            0,
            &mut info as *mut libc::proc_taskinfo as *mut libc::c_void,
            size,
        )
    };
    if written <= 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(info.pti_resident_size)
}

#[cfg(windows)]
pub fn resident_bytes(pid: u32) -> io::Result<u64> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    let mut counters: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
    counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    let ok = unsafe { GetProcessMemoryInfo(handle, &mut counters, counters.cb) };
    unsafe { CloseHandle(handle) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(counters.WorkingSetSize as u64)
}

/// Linux / other Unix: `VmRSS` from procfs. Exercised by the local gate; the
/// shipped desktop targets are macOS and Windows.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn resident_bytes(pid: u32) -> io::Result<u64> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status"))?;
    for line in status.lines() {
        let Some(rest) = line.strip_prefix("VmRSS:") else {
            continue;
        };
        let kb: u64 = rest
            .split_whitespace()
            .next()
            .and_then(|value| value.parse().ok())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "VmRSS is not a number"))?;
        return Ok(kb.saturating_mul(1024));
    }
    Err(io::Error::new(io::ErrorKind::NotFound, "VmRSS missing"))
}

#[cfg(not(any(unix, windows)))]
pub fn resident_bytes(pid: u32) -> io::Result<u64> {
    let _ = pid;
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "resident memory is not available on this platform",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_process_resident_memory_is_readable() {
        let bytes = resident_bytes(std::process::id()).expect("own rss must be readable");
        assert!(bytes > 0, "own rss must be positive, got {bytes}");
    }

    #[test]
    fn unreadable_pid_reports_an_error_instead_of_a_number() {
        // Never a live process: Linux has no procfs entry, macOS / Windows
        // refuse the query. Callers must degrade, not invent a figure.
        assert!(resident_bytes(u32::MAX).is_err());
    }
}
