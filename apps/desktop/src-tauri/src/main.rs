#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// SPDX-License-Identifier: GPL-3.0-or-later

fn main() {
    let mut args = std::env::args_os();
    let _exe = args.next();
    let bridge = args.next().is_some_and(|arg| arg == "--mcp-stdio");
    if bridge {
        std::process::exit(ice_box_lib::run_mcp_stdio_bridge(args));
    }
    ice_box_lib::run()
}
