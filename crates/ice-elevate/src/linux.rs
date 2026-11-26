// SPDX-License-Identifier: GPL-3.0-or-later

//! Linux elevation via Polkit (`pkexec`).
//!
//! Prompts the user through the desktop's PolicyKit authentication agent and
//! executes the narrow `ice-helper` installer binary as root with direct argv
//! passing (never through a shell). Exit status 126 means the user dismissed
//! the dialog (`ElevateError::Cancelled`); exit status 127 means Polkit denied
//! authorization or could not authenticate (`ElevateError::Denied`).

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{ElevateError, ElevateOutcome};

const PKEXEC_CANDIDATES: &[&str] = &["/usr/bin/pkexec", "/bin/pkexec"];

fn find_pkexec() -> Option<PathBuf> {
    for candidate in PKEXEC_CANDIDATES {
        let path = Path::new(candidate);
        if path.is_file() {
            return Some(path.to_path_buf());
        }
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join("pkexec");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Run `tool` with `args` as root via `pkexec`.
pub fn run_as_admin(tool: &Path, args: &[&str]) -> Result<ElevateOutcome, ElevateError> {
    let pkexec = find_pkexec().ok_or_else(|| {
        ElevateError::Unsupported(
            "pkexec (polkit) not found; install polkit or run scripts/install-helper-linux.sh with sudo"
                .to_string(),
        )
    })?;

    let output = Command::new(&pkexec)
        .arg(tool)
        .args(args)
        .output()
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                ElevateError::Unsupported(format!(
                    "pkexec not found at {}: {err}",
                    pkexec.display()
                ))
            } else {
                ElevateError::LaunchFailed(format!("spawn {}: {err}", pkexec.display()))
            }
        })?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    map_pkexec_output(output.status.code(), &stdout, &stderr)
}

/// Map `pkexec` exit status + captured streams onto the shared `ElevateOutcome`
/// contract (`OK ...` / `ERROR: ...` on stdout).
pub fn map_pkexec_output(
    status_code: Option<i32>,
    stdout: &str,
    stderr: &str,
) -> Result<ElevateOutcome, ElevateError> {
    let stderr_trimmed = stderr.trim();
    match status_code {
        // Polkit documented status 126: user dismissed the authentication dialog.
        Some(126) => Err(ElevateError::Cancelled),
        // Polkit documented status 127: not authorized / no agent / execution failed.
        Some(127) => {
            let detail = if stderr_trimmed.is_empty() {
                "polkit authorization failed (exit 127)".to_string()
            } else {
                stderr_trimmed.to_string()
            };
            Err(ElevateError::Denied(detail))
        }
        Some(code) => {
            let merged = merge_streams(stdout, stderr);
            if code != 0 && merged.trim().is_empty() {
                return Err(ElevateError::LaunchFailed(format!(
                    "elevated tool exited with status {code} and produced no output"
                )));
            }
            Ok(ElevateOutcome { output: merged })
        }
        None => {
            let detail = if stderr_trimmed.is_empty() {
                "elevated tool terminated by signal".to_string()
            } else {
                format!("elevated tool terminated by signal: {stderr_trimmed}")
            };
            Err(ElevateError::LaunchFailed(detail))
        }
    }
}

fn merge_streams(stdout: &str, stderr: &str) -> String {
    match (stdout.trim().is_empty(), stderr.trim().is_empty()) {
        (false, true) => stdout.to_string(),
        (true, false) => stderr.to_string(),
        (false, false) => {
            let mut merged = stdout.to_string();
            if !merged.ends_with('\n') {
                merged.push('\n');
            }
            merged.push_str(stderr);
            merged
        }
        (true, true) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_126_maps_to_cancelled() {
        let err = map_pkexec_output(
            Some(126),
            "",
            "Error executing command as another user: Request dismissed\n",
        )
        .unwrap_err();
        assert_eq!(err, ElevateError::Cancelled);
    }

    #[test]
    fn exit_127_maps_to_denied_with_stderr() {
        let err = map_pkexec_output(Some(127), "", "Not authorized\n").unwrap_err();
        assert!(matches!(err, ElevateError::Denied(msg) if msg.contains("Not authorized")));
    }

    #[test]
    fn ok_and_tool_error_lines_pass_through_to_outcome() {
        let ok = map_pkexec_output(Some(0), "OK helper installed\n", "").unwrap();
        assert_eq!(ok.output, "OK helper installed\n");

        let tool_err =
            map_pkexec_output(Some(1), "ERROR: systemd is not running\n", "extra\n").unwrap();
        assert!(tool_err
            .output
            .starts_with("ERROR: systemd is not running\n"));
        assert!(tool_err.output.contains("extra"));
    }

    #[test]
    fn silent_nonzero_and_signal_map_to_launch_failed() {
        let silent = map_pkexec_output(Some(1), "", "").unwrap_err();
        assert!(matches!(silent, ElevateError::LaunchFailed(_)));

        let signaled = map_pkexec_output(None, "", "killed").unwrap_err();
        assert!(matches!(signaled, ElevateError::LaunchFailed(_)));
    }
}
