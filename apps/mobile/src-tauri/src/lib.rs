// SPDX-License-Identifier: GPL-3.0-or-later

mod app_update;
mod commands;
mod config;
mod host;
mod status;

#[cfg(target_os = "android")]
mod android;

use std::sync::Mutex;

use tauri::Manager;

use crate::host::MobileHost;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_tunnel::init())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            app.manage(Mutex::new(MobileHost::new()));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::start,
            commands::stop,
            commands::list_subscriptions,
            commands::add_subscription,
            commands::import_subscription_file,
            commands::remove_subscription,
            commands::update_subscription,
            commands::set_active_subscription,
            commands::subscription_share,
            commands::list_nodes,
            commands::set_selected_node,
            commands::set_group_selection,
            commands::test_node_delay,
            commands::get_traffic_snapshot,
            commands::get_traffic_since,
            commands::get_log_view,
            commands::get_runtime_config,
            commands::get_settings,
            commands::save_settings,
            commands::set_proxy_mode,
            commands::get_rule_overview,
            commands::list_rules,
            commands::set_rule_disabled,
            commands::add_custom_rule,
            commands::remove_custom_rule,
            commands::check_app_update,
            commands::record_app_update_check,
            commands::open_app_download,
            commands::install_app_update,
            commands::request_battery_exemption,
            commands::open_network_settings,
            commands::open_vpn_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
