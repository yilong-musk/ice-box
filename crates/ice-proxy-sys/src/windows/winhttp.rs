//! WinHTTP default-proxy snapshot / apply / restore.

use serde::{Deserialize, Serialize};
use windows_sys::Win32::Foundation::{GlobalFree, ERROR_ACCESS_DENIED, ERROR_PRIVILEGE_NOT_HELD};
use windows_sys::Win32::Networking::WinHttp::{
    WinHttpGetDefaultProxyConfiguration, WinHttpSetDefaultProxyConfiguration,
    WINHTTP_ACCESS_TYPE_NAMED_PROXY, WINHTTP_PROXY_INFO,
};

use super::wide::{encode_wide, wide_ptr_to_string};
use crate::ProxySysError;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct WinHttpSnapshot {
    pub access_type: u32,
    #[serde(default)]
    pub proxy: Option<String>,
    #[serde(default)]
    pub bypass: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WinHttpWrite {
    Applied,
    AccessDenied,
}

fn is_privilege_error(err: &std::io::Error) -> bool {
    matches!(
        err.raw_os_error(),
        Some(code)
            if code == ERROR_ACCESS_DENIED as i32 || code == ERROR_PRIVILEGE_NOT_HELD as i32
    )
}

pub fn query_winhttp() -> Result<WinHttpSnapshot, ProxySysError> {
    let mut info = WINHTTP_PROXY_INFO {
        dwAccessType: 0,
        lpszProxy: std::ptr::null_mut(),
        lpszProxyBypass: std::ptr::null_mut(),
    };
    // SAFETY: `info` is a valid out-parameter; WinHTTP allocates proxy strings on success.
    let ok = unsafe { WinHttpGetDefaultProxyConfiguration(&mut info) };
    if ok == 0 {
        return Err(ProxySysError::Other(anyhow::anyhow!(
            "WinHttpGetDefaultProxyConfiguration: {}",
            std::io::Error::last_os_error()
        )));
    }
    let snapshot = WinHttpSnapshot {
        access_type: info.dwAccessType,
        proxy: wide_ptr_to_string(info.lpszProxy),
        bypass: wide_ptr_to_string(info.lpszProxyBypass),
    };
    // SAFETY: WinHTTP requires GlobalFree of the two strings it allocated.
    unsafe {
        if !info.lpszProxy.is_null() {
            GlobalFree(info.lpszProxy.cast());
        }
        if !info.lpszProxyBypass.is_null() {
            GlobalFree(info.lpszProxyBypass.cast());
        }
    }
    Ok(snapshot)
}

pub fn apply_winhttp(proxy: &str, bypass: &str) -> Result<WinHttpWrite, ProxySysError> {
    write_winhttp(&WinHttpSnapshot {
        access_type: WINHTTP_ACCESS_TYPE_NAMED_PROXY,
        proxy: Some(proxy.to_string()),
        bypass: Some(bypass.to_string()),
    })
}

pub fn restore_winhttp(snapshot: &WinHttpSnapshot) -> Result<WinHttpWrite, ProxySysError> {
    write_winhttp(snapshot)
}

fn write_winhttp(snapshot: &WinHttpSnapshot) -> Result<WinHttpWrite, ProxySysError> {
    let mut proxy_w = snapshot.proxy.as_deref().map(encode_wide);
    let mut bypass_w = snapshot.bypass.as_deref().map(encode_wide);
    let mut info = WINHTTP_PROXY_INFO {
        dwAccessType: snapshot.access_type,
        lpszProxy: proxy_w
            .as_mut()
            .map(|v| v.as_mut_ptr())
            .unwrap_or(std::ptr::null_mut()),
        lpszProxyBypass: bypass_w
            .as_mut()
            .map(|v| v.as_mut_ptr())
            .unwrap_or(std::ptr::null_mut()),
    };
    // SAFETY: wide buffers outlive this call; null proxy/bypass is valid for NO_PROXY.
    let ok = unsafe { WinHttpSetDefaultProxyConfiguration(&mut info) };
    if ok != 0 {
        return Ok(WinHttpWrite::Applied);
    }
    let err = std::io::Error::last_os_error();
    if is_privilege_error(&err) {
        return Ok(WinHttpWrite::AccessDenied);
    }
    Err(ProxySysError::ApplyFailed(format!(
        "WinHttpSetDefaultProxyConfiguration: {err}"
    )))
}
