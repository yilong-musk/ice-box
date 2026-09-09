// SPDX-License-Identifier: GPL-3.0-or-later

//! Named stop event for the elevated TUN core.
//!
//! A file in a user-writable directory is an unauthenticated kill switch and
//! a junction LPE primitive. This event lives in the Global namespace with a
//! DACL (SYSTEM + Administrators full; interactive user modify) and a Medium
//! integrity label so the unelevated app can `SetEvent`.

use std::os::windows::ffi::OsStrExt;

use windows_sys::Win32::Foundation::{
    CloseHandle, LocalFree, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::System::Threading::{
    CreateEventW, GetCurrentProcess, OpenProcessToken, ResetEvent, WaitForSingleObject,
};

pub struct StopEvent {
    handle: HANDLE,
}

impl StopEvent {
    pub fn create() -> Result<Self, ()> {
        let sid = current_user_sid_string().ok_or(())?;
        let sddl = format!(
            "O:BAD:P(A;;0x1F0003;;;SY)(A;;0x1F0003;;;BA)(A;;0x00100002;;;{sid})S:(ML;;NW;;;ME)"
        );
        let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        let wide = wide_z(&sddl);
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide.as_ptr(),
                SDDL_REVISION_1,
                &mut sd,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 || sd.is_null() {
            return Err(());
        }
        let attrs = windows_sys::Win32::Security::SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<windows_sys::Win32::Security::SECURITY_ATTRIBUTES>()
                as u32,
            lpSecurityDescriptor: sd,
            bInheritHandle: 0,
        };
        let name = wide_z(ice_tun_pin::TUN_STOP_EVENT_NAME);
        // Manual-reset: one SetEvent wakes the poll loop. ResetEvent clears a
        // leftover signaled object if CreateEventW opened an existing name
        // (lpEventAttributes is ignored for an already-created event).
        let handle = unsafe { CreateEventW(&attrs, 1, 0, name.as_ptr()) };
        unsafe {
            let _ = LocalFree(sd as _);
        }
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            return Err(());
        }
        unsafe {
            let _ = ResetEvent(handle);
        }
        Ok(Self { handle })
    }

    /// True when the unelevated app requested a graceful stop.
    pub fn wait_signaled(&self, timeout_ms: u32) -> bool {
        unsafe { WaitForSingleObject(self.handle, timeout_ms) == WAIT_OBJECT_0 }
    }
}

impl Drop for StopEvent {
    fn drop(&mut self) {
        if !self.handle.is_null() && self.handle != INVALID_HANDLE_VALUE {
            unsafe {
                let _ = CloseHandle(self.handle);
            }
        }
    }
}

fn current_user_sid_string() -> Option<String> {
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return None;
        }
        let mut needed = 0u32;
        let _ = GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut needed);
        if needed == 0 {
            let _ = CloseHandle(token);
            return None;
        }
        let mut buf = vec![0u8; needed as usize];
        let ok = GetTokenInformation(
            token,
            TokenUser,
            buf.as_mut_ptr() as *mut _,
            needed,
            &mut needed,
        );
        let _ = CloseHandle(token);
        if ok == 0 {
            return None;
        }
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut sid_str = std::ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut sid_str) == 0 || sid_str.is_null() {
            return None;
        }
        let mut len = 0usize;
        while *sid_str.add(len) != 0 {
            len += 1;
        }
        let slice = std::slice::from_raw_parts(sid_str, len);
        let value = String::from_utf16_lossy(slice);
        let _ = LocalFree(sid_str as _);
        Some(value)
    }
}

fn wide_z(value: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}
