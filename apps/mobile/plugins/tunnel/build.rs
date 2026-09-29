// SPDX-License-Identifier: GPL-3.0-or-later

const COMMANDS: &[&str] = &[
    "prepare",
    "start",
    "stop",
    "reload",
    "status",
    "shared_dir",
    "memory",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .build();
}
