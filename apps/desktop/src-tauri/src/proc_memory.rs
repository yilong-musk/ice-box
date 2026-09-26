// SPDX-License-Identifier: GPL-3.0-or-later

//! Per-process memory figure, for the Home memory readout.
//!
//! Scope (plan v0.1.14 §9, D5): the core process (sing-box) and the app's own
//! main process only. WebView helpers (WebKit / WebView2) and the privileged
//! helper daemon are out of scope, so the figure is lower than the "whole app"
//! number in Activity Monitor / Task Manager.
//!
//! Metric: both shipped platforms report only the process's own pages. macOS
//! reads the physical footprint (`ri_phys_footprint`), the value behind
//! Activity Monitor's "Memory" column. Windows reads the private working set
//! (`PROCESS_MEMORY_COUNTERS_EX2.PrivateWorkingSetSize`) and falls back to the
//! total working set on builds that predate that counter. An earlier macOS
//! revision used RSS (`pti_resident_size`), which also counts resident shared
//! framework pages and read roughly twice as high as Activity Monitor.
//! The two figures are not byte-for-byte equal — macOS adds compressed pages
//! and IOKit mappings, Windows excludes compressed pages — but they no longer
//! differ by the shared-page inflation RSS carried. Linux reads `VmRSS` from
//! procfs and is exercised by the local gate only.
//!
//! macOS degradation (D9a): a privileged core (helper / TUN) runs as root and
//! its memory cannot be read from the unprivileged app (`proc_pid_rusage`
//! fails with EPERM). That surfaces as an error here and the caller falls back
//! to the app-only figure — never elevated just to read it.

use std::io;

/// Memory footprint of `pid` in bytes.
///
/// Errors are expected (process gone, no permission): callers treat them as
/// "unreadable" and degrade the readout instead of inventing a number.
#[cfg(target_os = "macos")]
pub fn process_memory_bytes(pid: u32) -> io::Result<u64> {
    let mut info: libc::rusage_info_v4 = unsafe { std::mem::zeroed() };
    let rc = unsafe {
        libc::proc_pid_rusage(
            pid as libc::c_int,
            libc::RUSAGE_INFO_V4,
            &mut info as *mut libc::rusage_info_v4 as *mut libc::rusage_info_t,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(info.ri_phys_footprint)
}

/// Windows: the private working set — the process's own resident pages — with
/// the total working set as the fallback on builds that predate the counter
/// (Windows 10 22H2 / Windows 11 22H2, September 2023 cumulative update).
#[cfg(windows)]
pub fn process_memory_bytes(pid: u32) -> io::Result<u64> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX2,
    };
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    let mut counters: PROCESS_MEMORY_COUNTERS_EX2 = unsafe { std::mem::zeroed() };
    counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32;
    let ok = unsafe {
        GetProcessMemoryInfo(
            handle,
            &mut counters as *mut PROCESS_MEMORY_COUNTERS_EX2 as *mut PROCESS_MEMORY_COUNTERS,
            counters.cb,
        )
    };
    if ok != 0 && counters.PrivateWorkingSetSize > 0 {
        unsafe { CloseHandle(handle) };
        return Ok(counters.PrivateWorkingSetSize as u64);
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
pub fn process_memory_bytes(pid: u32) -> io::Result<u64> {
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
pub fn process_memory_bytes(pid: u32) -> io::Result<u64> {
    let _ = pid;
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "process memory is not available on this platform",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_process_memory_is_readable() {
        let bytes = process_memory_bytes(std::process::id()).expect("own memory must be readable");
        assert!(bytes > 0, "own memory must be positive, got {bytes}");
    }

    /// The Home row reads the core through the same cross-process path.
    #[cfg(unix)]
    #[test]
    fn same_user_child_process_memory_is_readable() {
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .expect("spawn a child process");
        let bytes = process_memory_bytes(child.id()).expect("child memory must be readable");
        assert!(bytes > 0, "child memory must be positive, got {bytes}");
        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn unreadable_pid_reports_an_error_instead_of_a_number() {
        // Never a live process: Linux has no procfs entry, macOS / Windows
        // refuse the query. Callers must degrade, not invent a figure.
        assert!(process_memory_bytes(u32::MAX).is_err());
    }
}
