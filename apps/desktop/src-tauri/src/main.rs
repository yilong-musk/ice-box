#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// SPDX-License-Identifier: GPL-3.0-or-later

fn main() {
    let mut args = std::env::args_os();
    let _exe = args.next();
    let bridge = args.next().is_some_and(|arg| arg == "--mcp-stdio");
    if bridge {
        std::process::exit(ice_box_lib::run_mcp_stdio_bridge(args));
    }
    #[cfg(target_os = "linux")]
    configure_linux_webkit_env();
    ice_box_lib::run()
}

/// Work around blank WebKitGTK windows on Linux Wayland + NVIDIA systems where
/// DMA-BUF rendering fails, without overriding an explicit user preference.
#[cfg(target_os = "linux")]
fn configure_linux_webkit_env() {
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_some() {
        return;
    }
    let is_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty())
        || std::env::var("XDG_SESSION_TYPE")
            .ok()
            .is_some_and(|v| v.eq_ignore_ascii_case("wayland"));
    let has_nvidia = std::path::Path::new("/proc/driver/nvidia/version").exists()
        || std::path::Path::new("/sys/module/nvidia").exists();
    if is_wayland && has_nvidia {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
}
