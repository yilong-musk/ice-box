// SPDX-License-Identifier: GPL-3.0-or-later

//! The Clash API endpoint is internal plumbing with no Settings field, so the
//! app owns its port. Before a core start, the shells call
//! [`relocate_clash_api_port`] and persist the result: when another program
//! holds the stored port (or it collides with the mixed port), the endpoint
//! moves to a free loopback port instead of failing the start.

use crate::health::{tcp_bind_available, tcp_port_is_in_use};
use ice_types::{AppError, AppSettings, ErrorCode};

/// Whether the core could bind the stored Clash API port right now.
///
/// Only meaningful while the app's own core is stopped: a live core holds the
/// port itself and would read as "in use".
pub fn clash_api_port_usable(settings: &AppSettings) -> bool {
    let host = settings.clash_api_listen.as_str();
    let port = settings.clash_api_port;
    port >= 1024
        && port != settings.mixed_port
        && !tcp_port_is_in_use(host, port)
        && tcp_bind_available(host, port)
}

/// Ask the OS for a free port on `host` (bind to port 0), skipping `avoid`
/// (the mixed port) and anything below 1024 that settings validation rejects.
pub fn pick_free_clash_api_port(host: &str, avoid: u16) -> Result<u16, AppError> {
    let mut last_err = String::from("no candidate port");
    for _ in 0..16 {
        match std::net::TcpListener::bind((host, 0)).and_then(|l| l.local_addr()) {
            Ok(addr) if addr.port() >= 1024 && addr.port() != avoid => return Ok(addr.port()),
            Ok(addr) => last_err = format!("OS returned unusable port {}", addr.port()),
            Err(err) => last_err = err.to_string(),
        }
    }
    Err(AppError::new(
        ErrorCode::CoreSpawnFailed,
        format!("could not find a free port for the clash API on {host}: {last_err}"),
    ))
}

/// `Some(updated)` when the Clash API port had to move; the caller persists
/// it so every later reader of `clash_api_port` agrees with the core it
/// starts. `None` when the stored port is usable as is.
pub fn relocate_clash_api_port(settings: &AppSettings) -> Result<Option<AppSettings>, AppError> {
    if clash_api_port_usable(settings) {
        return Ok(None);
    }
    let port = pick_free_clash_api_port(&settings.clash_api_listen, settings.mixed_port)?;
    tracing::info!(
        previous = settings.clash_api_port,
        port,
        "clash API port unavailable; moved to a free loopback port"
    );
    let mut updated = settings.clone();
    updated.clash_api_port = port;
    Ok(Some(updated))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn free_port() -> u16 {
        pick_free_clash_api_port("127.0.0.1", 0).unwrap()
    }

    fn settings_with_free_ports() -> AppSettings {
        let mixed_port = free_port();
        AppSettings {
            mixed_port,
            clash_api_port: pick_free_clash_api_port("127.0.0.1", mixed_port).unwrap(),
            ..AppSettings::default()
        }
    }

    #[test]
    fn keeps_a_free_port() {
        let settings = settings_with_free_ports();
        assert!(relocate_clash_api_port(&settings).unwrap().is_none());
    }

    #[test]
    fn moves_off_a_taken_port() {
        let holder = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let taken = holder.local_addr().unwrap().port();
        let settings = AppSettings {
            clash_api_port: taken,
            ..settings_with_free_ports()
        };
        let moved = relocate_clash_api_port(&settings).unwrap().expect("moved");
        assert_ne!(moved.clash_api_port, taken);
        assert_ne!(moved.clash_api_port, settings.mixed_port);
        moved.validate().unwrap();
        drop(holder);
    }

    #[test]
    fn moves_off_the_mixed_port() {
        let base = settings_with_free_ports();
        let settings = AppSettings {
            clash_api_port: base.mixed_port,
            ..base
        };
        let moved = relocate_clash_api_port(&settings).unwrap().expect("moved");
        assert_ne!(moved.clash_api_port, settings.mixed_port);
        moved.validate().unwrap();
    }

    #[test]
    fn picked_port_skips_the_avoided_port() {
        let port = free_port();
        assert!(port >= 1024);
        assert_ne!(pick_free_clash_api_port("127.0.0.1", port).unwrap(), port);
    }
}
