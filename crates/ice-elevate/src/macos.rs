//! AuthorizationServices FFI (macOS only).
//!
//! Uses the deprecated-but-functional `AuthorizationExecuteWithPrivileges`
//! root-execution path (see `crate::lib` for the rationale). The Security
//! framework is linked via the `#[link]` attribute; no third-party crate is
//! involved.

use std::ffi::CString;
use std::io::Read;
use std::os::unix::io::FromRawFd;
use std::path::Path;

use libc::{c_char, c_int, c_void};

use crate::{ElevateError, ElevateOutcome};

/// `errAuthorizationCanceled`
const ERR_AUTHORIZATION_CANCELED: c_int = -60006;

/// Right name that lets a normal user authorize with an admin password.
const RIGHT_ADMIN: &[u8] = b"system.privilege.admin\0";

const FLAG_DEFAULTS: u32 = 0;
const FLAG_INTERACTION_ALLOWED: u32 = 1 << 0;
const FLAG_EXTEND_RIGHTS: u32 = 1 << 1;

type AuthorizationRef = *mut c_void;
type OSStatus = c_int;

#[repr(C)]
struct AuthorizationItem {
    name: *const c_char,
    /// `size_t` in the SDK headers — must be pointer-width, not u32.
    value_length: usize,
    value: *mut c_void,
    flags: u32,
}

#[repr(C)]
struct AuthorizationRights {
    count: u32,
    items: *mut AuthorizationItem,
}

/// Layout locks against the SDK headers (64-bit): `AuthorizationItem` is
/// `{ const char*; size_t; void*; uint32 }` → 32 bytes; `AuthorizationRights`
/// is `{ uint32; pad; items* }` → 16 bytes. A wrong layout crashes inside
/// `xpc_data_create` (`_xpc_api_misuse`) when the framework reads past the
/// struct.
const _: () = assert!(std::mem::size_of::<AuthorizationItem>() == 32);
const _: () = assert!(std::mem::size_of::<AuthorizationRights>() == 16);

#[link(name = "Security", kind = "framework")]
extern "C" {
    fn AuthorizationCreate(
        rights: *const AuthorizationRights,
        environment: *const AuthorizationRights,
        flags: u32,
        authorization: *mut AuthorizationRef,
    ) -> OSStatus;

    fn AuthorizationCopyRights(
        authorization: AuthorizationRef,
        rights: *const AuthorizationRights,
        environment: *const AuthorizationRights,
        flags: u32,
        authorized_rights: *mut *mut AuthorizationRights,
    ) -> OSStatus;

    fn AuthorizationExecuteWithPrivileges(
        authorization: AuthorizationRef,
        path_to_tool: *const c_char,
        options: u32,
        arguments: *mut *mut c_char,
        communications_pipe: *mut *mut libc::FILE,
    ) -> OSStatus;

    fn AuthorizationFree(authorization: AuthorizationRef, flags: u32) -> OSStatus;
}

/// Build the NULL-terminated argv array; the `CString`s must stay alive for
/// the duration of the call (the caller keeps both in scope together).
fn argv_pointers(strings: &[CString]) -> Vec<*mut c_char> {
    let mut raw: Vec<*mut c_char> = strings.iter().map(|s| s.as_ptr() as *mut c_char).collect();
    raw.push(std::ptr::null_mut());
    raw
}

/// Reap the trampoline child AuthorizationServices spawns for us. The tool
/// itself is a grandchild, so this only cleans up the short-lived trampoline;
/// reaping at most one child keeps unrelated app children untouched.
fn reap_trampoline() {
    for _ in 0..100 {
        let mut status: c_int = 0;
        let pid = unsafe { libc::waitpid(-1, &mut status, libc::WNOHANG) };
        if pid == -1 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::ECHILD) {
                return;
            }
            // EINTR: retry; anything else: nothing more to reap.
            if err.raw_os_error() != Some(libc::EINTR) {
                return;
            }
        } else if pid > 0 {
            return;
        } else {
            // No exited child yet; the trampoline is still running or already
            // reaped. Poll briefly with a bounded total wait.
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
}

/// Read the tool's output pipe to EOF, then reap the trampoline. The pipe
/// blocks until the tool exits (and the trampoline closes its copy of the
/// fd), so EOF is the completion signal.
fn read_output(pipe: *mut libc::FILE) -> Result<String, ElevateError> {
    let fd = unsafe { libc::fileno(pipe) };
    if fd < 0 {
        return Err(ElevateError::Io("authorization pipe has no fd".into()));
    }
    // Take ownership of the fd (closes on drop) and skip the explicit
    // fclose on the FILE* to avoid a double close.
    let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|e| ElevateError::Io(format!("read pipe: {e}")))?;
    drop(file);
    reap_trampoline();
    let output = String::from_utf8_lossy(&bytes).to_string();
    Ok(output.trim_end().to_string())
}

fn status_message(context: &str, status: OSStatus) -> String {
    if status == ERR_AUTHORIZATION_CANCELED {
        return format!("{context}: canceled by user");
    }
    format!("{context}: OSStatus {status}")
}

/// See [`crate::run_as_admin`].
pub fn run_as_admin(tool: &Path, args: &[&str]) -> Result<ElevateOutcome, ElevateError> {
    let tool_c = CString::new(tool.as_os_str().as_encoded_bytes())
        .map_err(|_| ElevateError::LaunchFailed("tool path contains NUL byte".into()))?;
    let argv_owned = args
        .iter()
        .map(|arg| {
            CString::new(*arg)
                .map_err(|_| ElevateError::LaunchFailed("argument contains NUL byte".into()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let argv = argv_pointers(&argv_owned);

    let mut auth: AuthorizationRef = std::ptr::null_mut();
    let status = unsafe {
        AuthorizationCreate(std::ptr::null(), std::ptr::null(), FLAG_DEFAULTS, &mut auth)
    };
    if status != 0 {
        return Err(ElevateError::Denied(status_message(
            "AuthorizationCreate",
            status,
        )));
    }

    // One admin right; the dialog appears during CopyRights (interaction
    // allowed + extend rights).
    let mut item = AuthorizationItem {
        name: RIGHT_ADMIN.as_ptr() as *const c_char,
        value_length: 0,
        value: std::ptr::null_mut(),
        flags: 0,
    };
    let rights = AuthorizationRights {
        count: 1,
        items: &mut item,
    };

    let copy_status = unsafe {
        AuthorizationCopyRights(
            auth,
            &rights,
            std::ptr::null(),
            FLAG_INTERACTION_ALLOWED | FLAG_EXTEND_RIGHTS,
            std::ptr::null_mut(),
        )
    };
    if copy_status == ERR_AUTHORIZATION_CANCELED {
        unsafe { AuthorizationFree(auth, FLAG_DEFAULTS) };
        return Err(ElevateError::Cancelled);
    }
    if copy_status != 0 {
        unsafe { AuthorizationFree(auth, FLAG_DEFAULTS) };
        return Err(ElevateError::Denied(status_message(
            "AuthorizationCopyRights",
            copy_status,
        )));
    }

    // Execute the tool as root. The prompt is normally already settled by
    // CopyRights; a second prompt here would also surface as Cancelled.
    let mut pipe: *mut libc::FILE = std::ptr::null_mut();
    let exec_status = unsafe {
        AuthorizationExecuteWithPrivileges(
            auth,
            tool_c.as_ptr(),
            FLAG_DEFAULTS,
            argv.as_ptr() as *mut *mut c_char,
            &mut pipe,
        )
    };
    unsafe { AuthorizationFree(auth, FLAG_DEFAULTS) };
    if exec_status == ERR_AUTHORIZATION_CANCELED {
        return Err(ElevateError::Cancelled);
    }
    if exec_status != 0 {
        return Err(ElevateError::LaunchFailed(status_message(
            "AuthorizationExecuteWithPrivileges",
            exec_status,
        )));
    }
    if pipe.is_null() {
        return Err(ElevateError::LaunchFailed(
            "AuthorizationExecuteWithPrivileges returned no output pipe".to_string(),
        ));
    }

    Ok(ElevateOutcome {
        output: read_output(pipe)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argv_builder_is_null_terminated_and_ordered() {
        let owned: Vec<CString> = ["install", "/a b/c", "501"]
            .iter()
            .map(|s| CString::new(*s).expect("cstring"))
            .collect();
        let raw = argv_pointers(&owned);
        assert_eq!(owned.len(), 3);
        assert_eq!(raw.len(), 4);
        assert!(raw[3].is_null());
        for (c, r) in owned.iter().zip(raw.iter()) {
            assert_eq!(c.as_ptr() as *mut c_char, *r);
        }
    }

    #[test]
    fn status_message_distinguishes_cancel() {
        assert!(status_message("copy", ERR_AUTHORIZATION_CANCELED).contains("canceled"));
        assert!(!status_message("copy", 0).contains("canceled"));
        assert!(status_message("exec", 1).contains("OSStatus 1"));
    }

    #[test]
    fn error_code_constants_match_darwin_headers() {
        assert_eq!(ERR_AUTHORIZATION_CANCELED, -60006);
    }
}
