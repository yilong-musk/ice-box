// SPDX-License-Identifier: GPL-3.0-or-later

const COMMANDS: &[&str] = &[
    "prepare",
    "start",
    "stop",
    "reload",
    "status",
    "shared_dir",
    "memory",
    "device_status",
    "request_battery_exemption",
    "open_network_settings",
    "open_vpn_settings",
    "open_https_url",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .build();
}
