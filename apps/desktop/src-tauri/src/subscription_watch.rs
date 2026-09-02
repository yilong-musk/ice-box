//! Background auto-update of subscriptions flagged with `auto_update`.

use crate::commands;
use crate::orchestrate::current_settings;
use crate::AppState;
use ice_subscription::{
    AutoUpdateInterval, SubscriptionManager, SubscriptionMeta, SubscriptionPaths,
};
use std::time::Duration;
use tauri::{AppHandle, Manager};
use uuid::Uuid;

/// How often the watchdog wakes up to check for due auto-update subscriptions.
pub const AUTO_UPDATE_TICK: Duration = Duration::from_secs(60 * 60);

/// Grace period before the first due-check pass: lets the core auto-start and
/// startup recovery finish so a network refresh never races app initialization.
const STARTUP_GRACE: Duration = Duration::from_secs(30);

/// Startup retry window for the first due-check pass: a user action holding the
/// orchestrate lock briefly defers the pass, which retries a few times before
/// settling into the hourly loop.
const STARTUP_RETRIES: usize = 6;
const STARTUP_RETRY_DELAY: Duration = Duration::from_secs(10);

fn interval_of(meta: &SubscriptionMeta) -> Duration {
    meta.auto_update_interval
        .map(AutoUpdateInterval::duration)
        .unwrap_or_else(AutoUpdateInterval::default_duration)
}

/// A subscription is due when it has never been refreshed or its last refresh
/// is older than its configured [`AutoUpdateInterval`].
fn is_due(
    last_updated: Option<chrono::DateTime<chrono::Utc>>,
    now: chrono::DateTime<chrono::Utc>,
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

/// Ids of the auto-update subscriptions that are due for a refresh.
pub(crate) fn due_auto_update_ids(
    items: &[SubscriptionMeta],
    now: chrono::DateTime<chrono::Utc>,
) -> Vec<Uuid> {
    items
        .iter()
        .filter(|m| m.auto_update && is_due(m.last_updated, now, interval_of(m)))
        .map(|m| m.id)
        .collect()
}

/// Refresh every due auto-update subscription. Fetches (parallel, up to one
/// `FETCH_TIMEOUT`) run without the orchestrate lock so the background pass never
/// queues Start/Stop/Settings behind it; the lock is taken with `try_lock` for
/// the disk phase + Apply so a busy user operation simply defers the refresh.
/// Returns `false` when the pass was deferred (orchestrate busy); the caller
/// may retry shortly.
pub(crate) fn auto_update_due(state: &AppState, app: &AppHandle) -> bool {
    let paths = SubscriptionPaths::from_app(&state.paths);
    let mgr = SubscriptionManager::open(paths);
    let items = match mgr.list() {
        Ok(items) => items,
        Err(err) => {
            tracing::warn!(error = %err, "auto-update: load index failed");
            return true;
        }
    };
    let due = due_auto_update_ids(&items, chrono::Utc::now());
    if due.is_empty() {
        return true;
    }
    let fetched = mgr.fetch_ids(due);
    let Ok(_orch) = state.orchestrate.try_lock() else {
        tracing::debug!("auto-update: orchestrate busy, deferring apply");
        return false;
    };
    let results = mgr.apply_all(fetched);
    let updated = results.iter().filter(|(_, r)| r.is_ok()).count();
    let failed = results.len() - updated;
    tracing::info!(updated, failed, "auto-update subscriptions");
    let settings = current_settings(&state.paths).unwrap_or_default();
    if let Some(warning) = commands::apply_after_subscription_change(app, state, &settings) {
        tracing::warn!(
            code = %warning.code,
            error = %warning.message,
            "auto-update: apply warning"
        );
    }
    true
}

/// Poll due auto-update subscriptions for the app lifetime (independent of
/// frontend tab visibility). The first pass runs shortly after launch (after a
/// startup grace period) so subscriptions that went stale while the app was
/// closed refresh promptly instead of waiting for the first hourly tick.
pub fn spawn_subscription_watchdog(app: AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(STARTUP_GRACE);
        for _ in 0..STARTUP_RETRIES {
            let Some(state) = app.try_state::<AppState>() else {
                return;
            };
            if auto_update_due(state.inner(), &app) {
                break;
            }
            std::thread::sleep(STARTUP_RETRY_DELAY);
        }
        loop {
            std::thread::sleep(AUTO_UPDATE_TICK);
            let Some(state) = app.try_state::<AppState>() else {
                break;
            };
            auto_update_due(state.inner(), &app);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn meta(
        auto_update: bool,
        last_updated: Option<chrono::DateTime<chrono::Utc>>,
        interval: Option<AutoUpdateInterval>,
    ) -> SubscriptionMeta {
        SubscriptionMeta {
            id: Uuid::new_v4(),
            name: "t".into(),
            url: "https://example.com/s".into(),
            active: false,
            format: ice_subscription::SubscriptionFormat::SingBox,
            node_count: 1,
            group_count: 0,
            rule_count: 0,
            has_dns: false,
            parse_warnings: vec![],
            last_updated,
            last_error: None,
            etag: None,
            last_modified: None,
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
        // 24h cadence, only 2h old: not due yet despite auto_update.
        let slow = meta(
            true,
            Some(now - chrono::Duration::hours(2)),
            Some(AutoUpdateInterval::TwentyFourHours),
        );
        // 1h cadence, 2h old: due.
        let fast = meta(
            true,
            Some(now - chrono::Duration::hours(2)),
            Some(AutoUpdateInterval::OneHour),
        );
        // Legacy flag without a stored interval falls back to the default.
        let legacy = meta(true, Some(now - chrono::Duration::hours(2)), None);
        let items = vec![slow, fast.clone(), legacy.clone()];

        let due = due_auto_update_ids(&items, now);
        assert_eq!(due, vec![fast.id, legacy.id]);
    }
}
