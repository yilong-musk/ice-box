// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::proxy_terminal::{self as terminal, shell_proxy_host, validate_shell_proxy_host};

pub(crate) fn mixed_proxy_endpoint(state: &AppState) -> Result<(String, u16), AppError> {
    let settings = current_settings(&state.paths)?;
    let core = state.core_snapshot.load();
    let port = core
        .state
        .inbound_port
        .filter(|p| *p > 0)
        .unwrap_or(settings.mixed_port);
    if port == 0 {
        return Err(AppError::new(
            ErrorCode::ConfigInvalid,
            "mixed port is not configured",
        ));
    }
    let raw_host = core
        .state
        .inbound_host
        .as_deref()
        .filter(|h| !h.is_empty())
        .unwrap_or(settings.mixed_listen.as_str());
    // `mixed_listen` is unvalidated while `allow_lan` is on, so the host is
    // checked before it reaches any generated shell command.
    //
    // The settings fallback is deliberate: the port resolves while the proxy
    // service is stopped, so both the Home card and the tray keep offering the
    // session helpers — the command can be prepared before the service starts.
    let host = shell_proxy_host(raw_host);
    validate_shell_proxy_host(&host)?;
    Ok((host, port))
}

pub(crate) fn copy_proxy_terminal_command(state: &AppState) -> Result<(), AppError> {
    let (host, port) = mixed_proxy_endpoint(state)?;
    terminal::copy_shell_proxy_command(&host, port)
}

pub(crate) fn open_proxy_terminal_from_state(state: &AppState) -> Result<(), AppError> {
    let (host, port) = mixed_proxy_endpoint(state)?;
    terminal::open_proxy_terminal(&host, port)
}
