//! WinInet per-connection query / apply / notify.

use std::mem::size_of;

use serde::{Deserialize, Serialize};
use windows_sys::Win32::Networking::WinInet::{
    InternetQueryOptionW, InternetSetOptionW, INTERNET_OPTION_PER_CONNECTION_OPTION,
    INTERNET_OPTION_PROXY_SETTINGS_CHANGED, INTERNET_OPTION_REFRESH,
    INTERNET_OPTION_SETTINGS_CHANGED, INTERNET_PER_CONN_AUTOCONFIG_URL, INTERNET_PER_CONN_FLAGS,
    INTERNET_PER_CONN_FLAGS_UI, INTERNET_PER_CONN_OPTIONW, INTERNET_PER_CONN_OPTIONW_0,
    INTERNET_PER_CONN_OPTION_LISTW, INTERNET_PER_CONN_PROXY_BYPASS, INTERNET_PER_CONN_PROXY_SERVER,
    PROXY_TYPE_DIRECT, PROXY_TYPE_PROXY,
};

use super::wide::{encode_wide, wide_buf_to_string};
use crate::ProxySysError;

/// Manual proxy only. WPAD (`AUTO_DETECT`) and PAC (`AUTO_PROXY_URL`) outrank
/// `ProxyEnable` and would leave the browser on DIRECT.
pub const APPLY_FLAGS: u32 = PROXY_TYPE_PROXY | PROXY_TYPE_DIRECT;

const STR_BUF_CHARS: usize = 4096;

/// One WinInet connection (LAN when `name` is `None`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PerConnSnapshot {
    #[serde(default)]
    pub name: Option<String>,
    pub flags: u32,
    #[serde(default)]
    pub proxy_server: Option<String>,
    #[serde(default)]
    pub proxy_bypass: Option<String>,
    #[serde(default)]
    pub autoconfig_url: Option<String>,
}

fn option_dw(dw_option: u32, dw_value: u32) -> INTERNET_PER_CONN_OPTIONW {
    INTERNET_PER_CONN_OPTIONW {
        dwOption: dw_option,
        Value: INTERNET_PER_CONN_OPTIONW_0 { dwValue: dw_value },
    }
}

fn option_str(dw_option: u32, psz_value: windows_sys::core::PWSTR) -> INTERNET_PER_CONN_OPTIONW {
    INTERNET_PER_CONN_OPTIONW {
        dwOption: dw_option,
        Value: INTERNET_PER_CONN_OPTIONW_0 {
            pszValue: psz_value,
        },
    }
}

fn set_option(option: u32, buffer: *const core::ffi::c_void, length: u32) -> bool {
    // SAFETY: `buffer` is either null (zero-length notify) or a valid
    // `INTERNET_PER_CONN_OPTION_LISTW` that outlives this call.
    unsafe { InternetSetOptionW(std::ptr::null(), option, buffer, length) != 0 }
}

fn connection_psz(name: Option<&str>) -> (Option<Vec<u16>>, windows_sys::core::PWSTR) {
    match name {
        Some(n) => {
            let mut wide = encode_wide(n);
            let ptr = wide.as_mut_ptr();
            (Some(wide), ptr)
        }
        None => (None, std::ptr::null_mut()),
    }
}

fn conn_label(name: Option<&str>) -> &str {
    name.unwrap_or("LAN")
}

pub fn notify_settings_changed() -> Result<(), ProxySysError> {
    if !set_option(INTERNET_OPTION_SETTINGS_CHANGED, std::ptr::null(), 0) {
        return Err(ProxySysError::ApplyFailed(format!(
            "InternetSetOption(SETTINGS_CHANGED): {}",
            std::io::Error::last_os_error()
        )));
    }
    if !set_option(INTERNET_OPTION_PROXY_SETTINGS_CHANGED, std::ptr::null(), 0) {
        return Err(ProxySysError::ApplyFailed(format!(
            "InternetSetOption(PROXY_SETTINGS_CHANGED): {}",
            std::io::Error::last_os_error()
        )));
    }
    if !set_option(INTERNET_OPTION_REFRESH, std::ptr::null(), 0) {
        return Err(ProxySysError::ApplyFailed(format!(
            "InternetSetOption(REFRESH): {}",
            std::io::Error::last_os_error()
        )));
    }
    Ok(())
}

fn query_flags_option(connection: Option<&str>, option: u32) -> Result<u32, ProxySysError> {
    let (_keep, psz) = connection_psz(connection);
    let mut conn_option = option_dw(option, 0);
    let mut list = INTERNET_PER_CONN_OPTION_LISTW {
        dwSize: size_of::<INTERNET_PER_CONN_OPTION_LISTW>() as u32,
        pszConnection: psz,
        dwOptionCount: 1,
        dwOptionError: 0,
        pOptions: &mut conn_option,
    };
    let mut size = size_of::<INTERNET_PER_CONN_OPTION_LISTW>() as u32;
    // SAFETY: `list` / `conn_option` / `psz` are valid for the duration of the query.
    let ok = unsafe {
        InternetQueryOptionW(
            std::ptr::null(),
            INTERNET_OPTION_PER_CONNECTION_OPTION,
            (&mut list as *mut INTERNET_PER_CONN_OPTION_LISTW).cast(),
            &mut size,
        )
    };
    if ok == 0 {
        return Err(ProxySysError::Other(anyhow::anyhow!(
            "InternetQueryOption(FLAGS) for {}: {}",
            conn_label(connection),
            std::io::Error::last_os_error()
        )));
    }
    // SAFETY: FLAGS / FLAGS_UI store a `dwValue`.
    Ok(unsafe { conn_option.Value.dwValue })
}

fn query_string_option(
    connection: Option<&str>,
    option: u32,
) -> Result<Option<String>, ProxySysError> {
    let (_keep, psz) = connection_psz(connection);
    let mut buf = vec![0u16; STR_BUF_CHARS];
    let mut conn_option = option_str(option, buf.as_mut_ptr());
    let mut list = INTERNET_PER_CONN_OPTION_LISTW {
        dwSize: size_of::<INTERNET_PER_CONN_OPTION_LISTW>() as u32,
        pszConnection: psz,
        dwOptionCount: 1,
        dwOptionError: 0,
        pOptions: &mut conn_option,
    };
    let mut size = size_of::<INTERNET_PER_CONN_OPTION_LISTW>() as u32;
    // SAFETY: `buf` backs `pszValue` for the duration of the query.
    let ok = unsafe {
        InternetQueryOptionW(
            std::ptr::null(),
            INTERNET_OPTION_PER_CONNECTION_OPTION,
            (&mut list as *mut INTERNET_PER_CONN_OPTION_LISTW).cast(),
            &mut size,
        )
    };
    if ok == 0 {
        return Err(ProxySysError::Other(anyhow::anyhow!(
            "InternetQueryOption(string {option}) for {}: {}",
            conn_label(connection),
            std::io::Error::last_os_error()
        )));
    }
    Ok(wide_buf_to_string(&buf))
}

/// Effective WinInet flags (`INTERNET_PER_CONN_FLAGS`). May hide `AUTO_DETECT`
/// when WinINET decides the current network does not use WPAD.
///
/// Only used by live gates (`g4_3`) to assert apply cleared WPAD/PAC on the
/// effective FLAGS path, not the FLAGS_UI backup snapshot.
#[cfg(all(test, target_os = "windows"))]
pub fn query_effective_flags(connection: Option<&str>) -> Result<u32, ProxySysError> {
    query_flags_option(connection, INTERNET_PER_CONN_FLAGS)
}

/// Flags to snapshot for restore. Prefer `FLAGS_UI` (Internet Options checkboxes);
/// `FLAGS` can omit WPAD as an optimization and would drop the user's checkbox on
/// restore. Fall back to `FLAGS` on older IE / query failure.
fn query_flags_for_backup(connection: Option<&str>) -> Result<u32, ProxySysError> {
    match (
        query_flags_option(connection, INTERNET_PER_CONN_FLAGS_UI),
        query_flags_option(connection, INTERNET_PER_CONN_FLAGS),
    ) {
        (Ok(ui), Ok(effective)) => {
            if ui != effective {
                tracing::debug!(
                    connection = conn_label(connection),
                    flags_ui = ui,
                    flags_effective = effective,
                    "WinInet FLAGS_UI and FLAGS differ; backing up FLAGS_UI for restore"
                );
            }
            Ok(ui)
        }
        (Ok(ui), Err(_)) => Ok(ui),
        (Err(_), Ok(effective)) => Ok(effective),
        (Err(ui_err), Err(_)) => Err(ui_err),
    }
}

pub fn query_per_conn(connection: Option<&str>) -> Result<PerConnSnapshot, ProxySysError> {
    Ok(PerConnSnapshot {
        name: connection.map(str::to_string),
        flags: query_flags_for_backup(connection)?,
        proxy_server: query_string_option(connection, INTERNET_PER_CONN_PROXY_SERVER)?,
        proxy_bypass: query_string_option(connection, INTERNET_PER_CONN_PROXY_BYPASS)?,
        autoconfig_url: query_string_option(connection, INTERNET_PER_CONN_AUTOCONFIG_URL)?,
    })
}

/// `autoconfig_url`: `Some` writes the PAC URL (including `Some("")` to clear).
/// `None` leaves `AutoConfigURL` untouched.
pub fn apply_per_conn(
    connection: Option<&str>,
    server: &str,
    bypass: &str,
    flags: u32,
    autoconfig_url: Option<&str>,
) -> Result<(), ProxySysError> {
    let (_keep_name, psz) = connection_psz(connection);
    let mut server_w = encode_wide(server);
    let mut bypass_w = encode_wide(bypass);
    let mut pac_w = autoconfig_url.map(encode_wide);
    // MSDN: set/restore connection type via FLAGS; FLAGS_UI keeps Internet Options in sync.
    let mut options = vec![
        option_dw(INTERNET_PER_CONN_FLAGS, flags),
        option_dw(INTERNET_PER_CONN_FLAGS_UI, flags),
        option_str(INTERNET_PER_CONN_PROXY_SERVER, server_w.as_mut_ptr()),
        option_str(INTERNET_PER_CONN_PROXY_BYPASS, bypass_w.as_mut_ptr()),
    ];
    if let Some(ref mut pac) = pac_w {
        options.push(option_str(
            INTERNET_PER_CONN_AUTOCONFIG_URL,
            pac.as_mut_ptr(),
        ));
    }
    let list = INTERNET_PER_CONN_OPTION_LISTW {
        dwSize: size_of::<INTERNET_PER_CONN_OPTION_LISTW>() as u32,
        pszConnection: psz,
        dwOptionCount: options.len() as u32,
        dwOptionError: 0,
        pOptions: options.as_mut_ptr(),
    };
    if !set_option(
        INTERNET_OPTION_PER_CONNECTION_OPTION,
        (&list as *const INTERNET_PER_CONN_OPTION_LISTW).cast(),
        size_of::<INTERNET_PER_CONN_OPTION_LISTW>() as u32,
    ) {
        return Err(ProxySysError::ApplyFailed(format!(
            "InternetSetOption(PER_CONNECTION_OPTION) for {}: {}",
            conn_label(connection),
            std::io::Error::last_os_error()
        )));
    }
    Ok(())
}

pub fn restore_per_conn(snap: &PerConnSnapshot) -> Result<(), ProxySysError> {
    // WinInet has no "delete string option": empty server/bypass clears the mixed
    // address apply wrote. PAC is different — apply never writes AutoConfigURL, so
    // `None` must leave the leftover URL alone (same idea as registry delete_if_present).
    apply_per_conn(
        snap.name.as_deref(),
        snap.proxy_server.as_deref().unwrap_or(""),
        snap.proxy_bypass.as_deref().unwrap_or(""),
        snap.flags,
        snap.autoconfig_url.as_deref(),
    )
}
