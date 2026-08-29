//! macOS elevation via AuthorizationServices for the unsigned release.
//!
//! The app never supports code signing or notarization (documented product
//! decision). Elevation therefore cannot use the signed-only SMAppService /
//! SMJobBless path; instead the app prompts the user with the system
//! authorization dialog and executes the bundled `ice-helper` tool as root:
//!
//! ```text
//! AuthorizationCreate
//!   -> AuthorizationCopyRights("system.privilege.admin",
//!                              interaction allowed | extend rights)  [password dialog]
//!   -> AuthorizationExecuteWithPrivileges(tool, args)                [tool runs as root]
//! ```
//!
//! `AuthorizationExecuteWithPrivileges` is deprecated since macOS 10.7 but
//! remains functional and is the only root-execution path that works for an
//! unsigned, non-sandboxed app. The design deliberately keeps the main app
//! process unelevated: the dialog grants root only to the small, narrow
//! installer mode of `ice-helper` (`install` / `uninstall`), never to the
//! desktop process itself.
//!
//! The elevated tool's stdout is captured until EOF (the tool must print a
//! single result line, see the `ice-helper` installer modes). The tool is not
//! a direct child of the caller, so the exit code cannot be read reliably;
//! callers parse the printed outcome instead.
//!
//! Non-macOS platforms get a fail-closed stub ([`ElevateError::Unsupported`]).

use std::path::Path;

/// Outcome of one elevated tool run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElevateOutcome {
    /// Tool output (stdout + stderr merged by AuthorizationServices).
    pub output: String,
}

/// Failure modes of [`run_as_admin`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElevateError {
    /// The user declined the system authorization dialog.
    Cancelled,
    /// The authorization request was denied or could not be completed.
    Denied(String),
    /// Elevation is not available on this platform / configuration.
    Unsupported(String),
    /// The elevated tool could not be launched (OSStatus + detail).
    LaunchFailed(String),
    /// I/O while capturing the tool output.
    Io(String),
}

impl std::fmt::Display for ElevateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => write!(f, "user cancelled the authorization dialog"),
            Self::Denied(msg) => write!(f, "authorization denied: {msg}"),
            Self::Unsupported(msg) => write!(f, "elevation unsupported: {msg}"),
            Self::LaunchFailed(msg) => write!(f, "launching elevated tool failed: {msg}"),
            Self::Io(msg) => write!(f, "reading elevated tool output failed: {msg}"),
        }
    }
}

impl std::error::Error for ElevateError {}

/// Prompt for root authorization (system password dialog) and run `tool` with
/// `args` as root. Blocks until the tool exits. The caller parses `output`
/// for the tool's printed result; the exit code is not available through the
/// AuthorizationServices pipe (see module docs).
#[cfg(target_os = "macos")]
pub fn run_as_admin(tool: &Path, args: &[&str]) -> Result<ElevateOutcome, ElevateError> {
    crate::macos::run_as_admin(tool, args)
}

/// Non-macOS stub: fail closed, nothing is ever executed elevated.
#[cfg(not(target_os = "macos"))]
pub fn run_as_admin(_tool: &Path, _args: &[&str]) -> Result<ElevateOutcome, ElevateError> {
    Err(ElevateError::Unsupported(
        "privileged helper installation is macOS-only".to_string(),
    ))
}

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_is_fail_closed_on_other_platforms() {
        #[cfg(not(target_os = "macos"))]
        {
            let err = run_as_admin(Path::new("/bin/true"), &[]).unwrap_err();
            assert!(matches!(err, ElevateError::Unsupported(_)));
        }
    }

    #[test]
    fn error_messages_are_stable_and_actionable() {
        let cases = [
            (ElevateError::Cancelled, "user cancelled"),
            (
                ElevateError::Denied("no rights".into()),
                "authorization denied: no rights",
            ),
            (
                ElevateError::Unsupported("macOS only".into()),
                "elevation unsupported: macOS only",
            ),
            (
                ElevateError::LaunchFailed("status -60006".into()),
                "launching elevated tool failed: status -60006",
            ),
        ];
        for (err, expected) in cases {
            assert!(err.to_string().contains(expected), "{err}");
        }
    }
}
