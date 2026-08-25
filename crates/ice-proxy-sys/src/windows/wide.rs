//! UTF-16 helpers for WinInet / WinHTTP string buffers.

use windows_sys::core::PWSTR;

pub fn encode_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn wide_buf_to_string(buf: &[u16]) -> Option<String> {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let s = String::from_utf16_lossy(&buf[..len]);
    let s = s.trim();
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

pub fn wide_ptr_to_string(ptr: PWSTR) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: caller guarantees `ptr` is a valid NUL-terminated PWSTR.
    let len = unsafe {
        let mut n = 0usize;
        while *ptr.add(n) != 0 {
            n += 1;
        }
        n
    };
    // SAFETY: `ptr` points at `len` UTF-16 code units.
    let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
    wide_buf_to_string(slice)
}
