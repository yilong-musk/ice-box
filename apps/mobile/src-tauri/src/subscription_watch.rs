// SPDX-License-Identifier: GPL-3.0-or-later

//! Foreground auto-update of subscriptions flagged with `auto_update`.
//!
//! The timer runs only while the activity is resumed. Bringing the app back
//! runs a due pass immediately. Background scheduling (`WorkManager`) is later.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use ice_subscription::{
    AutoUpdateInterval, SubscriptionManager, SubscriptionMeta, SubscriptionPaths,
};
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

use crate::config::PLATFORM;
use crate::host::MobileHost;

/// How often a resumed app looks for due subscriptions. Matches the desktop
/// watchdog so a one-hour subscription is not refreshed more often than that.
const TICK: Duration = Duration::from_secs(60 * 60);

/// Paths are created by the first UI command. A resume that races that call
/// waits once, then tries again.
const PATHS_WAIT: Duration = Duration::from_secs(5);

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub struct SubscriptionWatch {
    paused: Arc<AtomicBool>,
    wake: Mutex<mpsc::Sender<()>>,
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
impl SubscriptionWatch {
    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
        self.poke();
    }

    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
        self.poke();
    }

    fn poke(&self) {
        if let Ok(sender) = self.wake.lock() {
            let _ = sender.send(());
        }
    }
}

pub fn start(app: AppHandle) -> SubscriptionWatch {
    let (sender, receiver) = mpsc::channel();
    // The activity's first onResume is not delivered, and opening the app is
    // what starts this process. Begin in the foreground; pause only after
    // the system reports the activity was suspended.
    let paused = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&paused);
    std::thread::Builder::new()
        .name("subscription-update".into())
        .spawn(move || run_loop(app, receiver, flag))
        .expect("subscription-update thread");
    SubscriptionWatch {
        paused,
        wake: Mutex::new(sender),
    }
}

fn run_loop(app: AppHandle, wake: mpsc::Receiver<()>, paused: Arc<AtomicBool>) {
    loop {
        while paused.load(Ordering::SeqCst) {
            if wake.recv().is_err() {
                return;
            }
        }
        if !paths_ready(&app) {
            let _ = wake.recv_timeout(PATHS_WAIT);
            if paused.load(Ordering::SeqCst) {
                continue;
            }
        }
        refresh(&app);
        loop {
            match wake.recv_timeout(TICK) {
                Ok(()) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if !paused.load(Ordering::SeqCst) {
                        refresh(&app);
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
            if paused.load(Ordering::SeqCst) {
                break;
            }
        }
    }
}

fn paths_ready(app: &AppHandle) -> bool {
    app.state::<Mutex<MobileHost>>()
        .lock()
        .map(|host| host.paths_ready())
        .unwrap_or(false)
}

fn refresh(app: &AppHandle) {
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| refresh_due(app))).is_err() {
        log::error!("auto-update pass panicked");
    }
}

fn refresh_due(app: &AppHandle) {
    let host_state = app.state::<Mutex<MobileHost>>();
    let Some(paths) = (|| {
        let host = host_state.lock().ok()?;
        if !host.paths_ready() {
            return None;
        }
        host.app_paths().ok()
    })() else {
        return;
    };
    let manager = SubscriptionManager::open(SubscriptionPaths::from_app(&paths), PLATFORM);
    let items = match manager.list() {
        Ok(items) => items,
        Err(err) => {
            log::warn!("auto-update: load index failed: {err}");
            return;
        }
    };
    let due = due_auto_update_ids(&items, Utc::now());
    if due.is_empty() {
        return;
    }
    log::info!("auto-update: refreshing {} subscriptions", due.len());
    let fetched = manager.fetch_ids(due);
    let tunnel = app.state::<tauri_plugin_tunnel::Tunnel<tauri::Wry>>();
    let snapshot = tunnel.status();
    {
        let mut host = match host_state.lock() {
            Ok(host) => host,
            Err(_) => return,
        };
        let manager = SubscriptionManager::open(SubscriptionPaths::from_app(&paths), PLATFORM);
        let results = manager.apply_all(fetched);
        let updated = results.iter().filter(|(_, result)| result.is_ok()).count();
        let failed = results.len() - updated;
        log::info!("auto-update subscriptions: {updated} updated, {failed} failed");
        if updated > 0 {
            let rewrite = match &snapshot {
                Ok(status) => {
                    let package =
                        Some(status.package_name.as_str()).filter(|name| !name.is_empty());
                    host.rewrite_config(package)
                }
                Err(tauri_plugin_tunnel::TunnelError::Unavailable) => host.rewrite_config(None),
                Err(err) => {
                    log::warn!("auto-update: tunnel status failed: {err}");
                    Ok(())
                }
            };
            if let Err(err) = rewrite {
                log::warn!("auto-update: rewrite config failed: {err}");
            }
        }
    }
    if let Ok(status) = &snapshot {
        let live = crate::status::TunnelPhase::parse(&status.phase).is_live();
        if live {
            if let Err(err) = tunnel.reload() {
                log::warn!("auto-update: reload failed: {err}");
            }
        }
    }
    let _ = app.emit("core://status-changed", ());
    let _ = app.emit("app://state-changed", ());
}

fn interval_of(meta: &SubscriptionMeta) -> Duration {
    meta.auto_update_interval
        .map(AutoUpdateInterval::duration)
        .unwrap_or_else(AutoUpdateInterval::default_duration)
}

fn is_due(
    last_updated: Option<chrono::DateTime<Utc>>,
    now: chrono::DateTime<Utc>,
    interval: Duration,
) -> bool {
    match last_updated {
        None => true,
        Some(at) => {
            let interval =
                chrono::Duration::from_std(interval).unwrap_or_else(|_| chrono::Duration::zero());
            now.signed_duration_since(at) >= interval
        }
    }
}

pub(crate) fn due_auto_update_ids(
    items: &[SubscriptionMeta],
    now: chrono::DateTime<Utc>,
) -> Vec<Uuid> {
    items
        .iter()
        .filter(|meta| meta.auto_update && is_due(meta.last_updated, now, interval_of(meta)))
        .map(|meta| meta.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ice_subscription::SubscriptionFormat;

    fn meta(
        auto_update: bool,
        last_updated: Option<chrono::DateTime<Utc>>,
        interval: Option<AutoUpdateInterval>,
    ) -> SubscriptionMeta {
        SubscriptionMeta {
            id: Uuid::new_v4(),
            name: "t".into(),
            url: "https://example.com/s".into(),
            active: false,
            format: SubscriptionFormat::SingBox,
            node_count: 1,
            group_count: 0,
            rule_count: 0,
            has_dns: false,
            parse_warnings: vec![],
            last_updated,
            last_error: None,
            etag: None,
            last_modified: None,
            userinfo: None,
            provider_info: vec![],
            auto_update,
            auto_update_interval: interval,
        }
    }

    #[test]
    fn due_ids_skip_disabled_and_fresh_subscriptions() {
        let now = Utc::now();
        let fresh = meta(true, Some(now), None);
        let stale = meta(true, Some(now - chrono::Duration::hours(2)), None);
        let never = meta(true, None, None);
        let disabled = meta(false, Some(now - chrono::Duration::hours(2)), None);
        let items = vec![fresh, stale.clone(), never.clone(), disabled];

        let due = due_auto_update_ids(&items, now);
        assert_eq!(due, vec![stale.id, never.id]);
    }

    #[test]
    fn due_ids_respect_each_subscriptions_interval() {
        let now = Utc::now();
        let slow = meta(
            true,
            Some(now - chrono::Duration::hours(2)),
            Some(AutoUpdateInterval::TwentyFourHours),
        );
        let fast = meta(
            true,
            Some(now - chrono::Duration::hours(2)),
            Some(AutoUpdateInterval::OneHour),
        );
        let legacy = meta(true, Some(now - chrono::Duration::hours(2)), None);
        let items = vec![slow, fast.clone(), legacy.clone()];

        let due = due_auto_update_ids(&items, now);
        assert_eq!(due, vec![fast.id, legacy.id]);
    }
}
