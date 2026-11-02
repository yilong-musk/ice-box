// SPDX-License-Identifier: GPL-3.0-or-later

//! Bounded retry for spawning a transiently locked executable.
//!
//! An in-place app update or an antivirus scanner may still hold the freshly
//! replaced `sing-box` binary when the app relaunches. Windows reports a
//! sharing violation or access denied from `CreateProcess`; Unix reports
//! `ETXTBSY` while the file is open for writing. Both are transient, so the
//! spawn is retried with backoff before the original error is surfaced.

use std::io;
use std::process::Child;
use std::time::Duration;

/// Delays between spawn attempts while the executable is transiently locked.
/// Total added wait is ~7.1s; after that the original spawn error is surfaced.
pub const LOCK_RETRY_DELAYS: [Duration; 7] = [
    Duration::from_millis(100),
    Duration::from_millis(200),
    Duration::from_millis(400),
    Duration::from_millis(800),
    Duration::from_millis(1600),
    Duration::from_millis(2000),
    Duration::from_millis(2000),
];

/// Whether a failed spawn is worth retrying because the executable is still
/// being released. Windows reports a sharing violation (32) or access denied
/// (5) while an updater replaces the file or an antivirus scanner inspects it;
/// Unix reports ETXTBSY while the file is open for writing.
pub fn is_transient_spawn_lock(err: &io::Error) -> bool {
    #[cfg(windows)]
    {
        const ERROR_ACCESS_DENIED: i32 = 5;
        const ERROR_SHARING_VIOLATION: i32 = 32;
        matches!(
            err.raw_os_error(),
            Some(ERROR_ACCESS_DENIED) | Some(ERROR_SHARING_VIOLATION)
        )
    }
    #[cfg(unix)]
    {
        err.raw_os_error() == Some(libc::ETXTBSY)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = err;
        false
    }
}

/// Spawn via `spawn`, retrying transient lock errors with [`LOCK_RETRY_DELAYS`].
///
/// `spawn` runs once per attempt so it can rebuild the [`Command`]: a failed
/// spawn consumes the redirected stdio handles, and the retry must reopen the
/// log. Non-lock errors are returned immediately, and the last error is
/// returned unchanged once the retry budget is exhausted.
///
/// The backoff sleeps on the calling thread and is not cancellable. Callers
/// must run it on a worker thread (or `spawn_blocking`), where a concurrent
/// stop/quit can wait for at most the retry budget.
pub fn spawn_with_lock_retry<F>(spawn: F) -> io::Result<Child>
where
    F: FnMut() -> io::Result<Child>,
{
    spawn_with_delays(spawn, &LOCK_RETRY_DELAYS)
}

fn spawn_with_delays<F>(mut spawn: F, delays: &[Duration]) -> io::Result<Child>
where
    F: FnMut() -> io::Result<Child>,
{
    let mut retries = 0usize;
    loop {
        match spawn() {
            Ok(child) => return Ok(child),
            Err(err) => {
                if !is_transient_spawn_lock(&err) {
                    return Err(err);
                }
                let Some(delay) = delays.get(retries).copied() else {
                    tracing::warn!(
                        attempts = retries + 1,
                        error = %err,
                        "executable stayed locked; giving up"
                    );
                    return Err(err);
                };
                retries += 1;
                if retries == 1 {
                    tracing::warn!(
                        attempt = retries,
                        error = %err,
                        "executable is temporarily locked; retrying spawn"
                    );
                } else {
                    tracing::debug!(
                        attempt = retries,
                        error = %err,
                        "executable is still locked; retrying spawn"
                    );
                }
                std::thread::sleep(delay);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn locked_error() -> io::Error {
        io::Error::from_raw_os_error(libc::ETXTBSY)
    }

    #[cfg(windows)]
    fn locked_error() -> io::Error {
        io::Error::from_raw_os_error(32)
    }

    #[cfg(unix)]
    #[test]
    fn classifier_matches_etxtbsy_only() {
        assert!(is_transient_spawn_lock(&locked_error()));
        assert!(!is_transient_spawn_lock(&io::Error::from_raw_os_error(
            libc::ENOENT
        )));
    }

    #[cfg(windows)]
    #[test]
    fn classifier_matches_windows_sharing_errors() {
        assert!(is_transient_spawn_lock(&io::Error::from_raw_os_error(32)));
        assert!(is_transient_spawn_lock(&io::Error::from_raw_os_error(5)));
        assert!(!is_transient_spawn_lock(&io::Error::from_raw_os_error(2)));
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn retries_until_the_budget_is_exhausted() {
        let mut calls = 0usize;
        let err = spawn_with_delays(
            || {
                calls += 1;
                Err(locked_error())
            },
            &[Duration::ZERO, Duration::ZERO],
        )
        .expect_err("all attempts are locked");
        assert_eq!(calls, 3, "one initial attempt plus one per delay");
        assert_eq!(err.raw_os_error(), locked_error().raw_os_error());
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn non_lock_errors_fail_without_retrying() {
        let mut calls = 0usize;
        let result = spawn_with_delays(
            || {
                calls += 1;
                #[cfg(unix)]
                let err = io::Error::from_raw_os_error(libc::EACCES);
                #[cfg(not(unix))]
                let err = io::Error::from_raw_os_error(2);
                Err(err)
            },
            &[Duration::ZERO, Duration::ZERO],
        );
        assert!(result.is_err());
        assert_eq!(calls, 1, "non-lock errors must not consume the budget");
    }

    #[cfg(unix)]
    #[test]
    fn succeeds_after_transient_failures() {
        let mut calls = 0usize;
        let mut child = spawn_with_delays(
            || {
                calls += 1;
                if calls < 3 {
                    return Err(locked_error());
                }
                std::process::Command::new("/bin/sh")
                    .arg("-c")
                    .arg("exit 0")
                    .spawn()
            },
            &[Duration::ZERO, Duration::ZERO],
        )
        .expect("third attempt succeeds");
        assert_eq!(calls, 3);
        let _ = child.wait();
    }
}
