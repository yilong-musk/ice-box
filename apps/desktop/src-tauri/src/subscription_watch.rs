//! Background auto-update of subscriptions flagged with `auto_update`.

use crate::commands;
use crate::orchestrate::current_settings;
use crate::AppState;
use ice_subscription::{SubscriptionManager, SubscriptionMeta, SubscriptionPaths};
use std::time::Duration;
use tauri::{AppHandle, Manager};
use uuid::Uuid;

/// How often the watchdog checks due auto-update subscriptions.
pub const AUTO_UPDATE_INTERVAL: Duration = Duration::from_secs(30 * 60);

/// A subscription is due when it has never been refreshed or its last refresh
/// is older than one [`AUTO_UPDATE_INTERVAL`].
fn is_due(
    last_updated: Option<chrono::DateTime<chrono::Utc>>,
    now: chrono::DateTime<chrono::Utc>,
) -> bool {
    match last_updated {
        None => true,
        Some(at) => {
            let interval = chrono::Duration::from_std(AUTO_UPDATE_INTERVAL)
                .unwrap_or_else(|_| chrono::Duration::zero());
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
        .filter(|m| m.auto_update && is_due(m.last_updated, now))
        .map(|m| m.id)
        .collect()
}

/// Refresh every due auto-update subscription. Fetches (parallel, up to one
/// `FETCH_TIMEOUT`) run without the orchestrate lock so the background pass never
/// queues Start/Stop/Settings behind it; the lock is taken with `try_lock` for
/// the disk phase + Apply so a busy user operation simply defers the refresh to
/// the next tick.
pub(crate) fn auto_update_due(state: &AppState, app: &AppHandle) {
    let paths = SubscriptionPaths::from_app(&state.paths);
    let mgr = SubscriptionManager::open(paths);
    let items = match mgr.list() {
        Ok(items) => items,
        Err(err) => {
            tracing::warn!(error = %err, "auto-update: load index failed");
            return;
        }
    };
    let due = due_auto_update_ids(&items, chrono::Utc::now());
    if due.is_empty() {
        return;
    }
    let fetched = mgr.fetch_ids(due);
    let Ok(_orch) = state.orchestrate.try_lock() else {
        tracing::debug!("auto-update: orchestrate busy, deferring apply");
        return;
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
}

/// Poll due auto-update subscriptions for the app lifetime (independent of
/// frontend tab visibility).
pub fn spawn_subscription_watchdog(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(AUTO_UPDATE_INTERVAL);
        let Some(state) = app.try_state::<AppState>() else {
            break;
        };
        auto_update_due(state.inner(), &app);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn meta(
        auto_update: bool,
        last_updated: Option<chrono::DateTime<chrono::Utc>>,
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
        }
    }

    #[test]
    fn due_ids_skip_disabled_and_fresh_subscriptions() {
        let now = Utc::now();
        let fresh = meta(true, Some(now));
        let stale = meta(true, Some(now - chrono::Duration::hours(2)));
        let never = meta(true, None);
        let disabled = meta(false, Some(now - chrono::Duration::hours(2)));
        let items = vec![fresh, stale.clone(), never.clone(), disabled];

        let due = due_auto_update_ids(&items, now);
        assert_eq!(due, vec![stale.id, never.id]);
    }
}
