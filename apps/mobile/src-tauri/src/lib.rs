// SPDX-License-Identifier: GPL-3.0-or-later

mod app_update;
mod commands;
mod config;
mod host;
mod status;
mod subscription_watch;

#[cfg(target_os = "android")]
mod android;

use std::sync::Mutex;

use tauri::Manager;
#[cfg(target_os = "android")]
use tauri::RunEvent;

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
            app.manage(subscription_watch::start(app.handle().clone()));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::start,
            commands::stop,
            commands::list_subscriptions,
            commands::add_subscription,
            commands::remove_subscription,
            commands::update_subscription,
            commands::update_all_subscriptions,
            commands::set_active_subscription,
            commands::set_auto_update_subscription,
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
            commands::request_battery_exemption,
            commands::open_network_settings,
            commands::open_vpn_settings,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // Activity onPause / a later onResume. The first onResume is not
            // delivered; the watcher starts in the foreground instead.
            #[cfg(target_os = "android")]
            if let Some(watch) = app.try_state::<subscription_watch::SubscriptionWatch>() {
                match event {
                    RunEvent::WindowEvent {
                        event: tauri::WindowEvent::Suspended,
                        ..
                    } => watch.pause(),
                    RunEvent::WindowEvent {
                        event: tauri::WindowEvent::Resumed,
                        ..
                    } => watch.resume(),
                    _ => {}
                }
            }
            #[cfg(not(target_os = "android"))]
            {
                let _ = (app, event);
            }
        });
}
