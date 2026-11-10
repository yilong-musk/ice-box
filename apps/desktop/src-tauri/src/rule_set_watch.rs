// SPDX-License-Identifier: GPL-3.0-or-later

//! Background download of the remote rule-sets the active profile needs.
//!
//! The core never fetches rule-sets; this worker does, with the subscription
//! fetcher (system proxy, SSRF checks), and caches sanitized copies under
//! `AppPaths::rule_set_cache_dir`. The config builder picks them up on the
//! next Apply; when a pass downloads new content it re-applies right away.
//!
//! Cached copies are re-checked only with the subscriptions: a full pass
//! ([`RuleSetRefreshMode::All`]) follows every subscription update (manual
//! or automatic) and the manual "refresh rule-sets". Every other pass
//! (shortly after launch, every [`TICK`], after a switch or removal) only
//! downloads sets that have no cached copy yet, so a set that failed to
//! download is retried without re-checking the others.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use ice_engine::RuleSetRefreshMode;
use tauri::{AppHandle, Manager};

use crate::application;
use crate::orchestrate::current_settings;
use crate::AppState;

const STARTUP_GRACE: Duration = Duration::from_secs(20);
const TICK: Duration = Duration::from_secs(60 * 60);
/// How often the worker checks for a requested pass.
const POLL: Duration = Duration::from_secs(3);

static REQUESTED: AtomicBool = AtomicBool::new(false);
/// A requested pass should re-check every cached copy.
static FULL: AtomicBool = AtomicBool::new(false);
/// New content was downloaded but not applied yet (Apply was busy). Kept
/// apart from the download result: the retry finds every URL fresh and
/// would otherwise never apply.
static APPLY_PENDING: AtomicBool = AtomicBool::new(false);

/// Ask the worker for a pass soon. Returns at once, so a subscription
/// update never waits for rule-set downloads.
pub(crate) fn request_refresh(mode: RuleSetRefreshMode) {
    if mode == RuleSetRefreshMode::All {
        FULL.store(true, Ordering::SeqCst);
    }
    REQUESTED.store(true, Ordering::SeqCst);
}

/// URLs the active profile and the custom rules need downloaded.
fn required_urls(state: &AppState) -> Option<Vec<String>> {
    let mgr = application::subscription_manager(state).ok()?;
    let profile = match mgr.active_profile() {
        Ok(profile) => profile,
        Err(err) => {
            tracing::debug!(error = %err.redacted_display(), "rule-set refresh: no active profile");
            return Some(Vec::new());
        }
    };
    let custom = ice_config::load_rule_overrides(&state.paths.rule_overrides()).custom;
    let mut rules = profile.route.rules.clone();
    rules.extend(custom);
    Some(ice_config::required_rule_set_urls(
        &profile.route.rule_sets,
        &rules,
        profile.dns.as_ref(),
        Some(&state.paths.geoip_dir()),
    ))
}

/// One refresh pass. Returns `false` when it should be retried soon (the
/// manager could not be opened or Apply was busy).
pub(crate) fn refresh_pass(
    state: &AppState,
    app: &impl application::AppHost,
    mode: RuleSetRefreshMode,
) -> bool {
    if state.workers.is_paused_or_stopping() {
        return true;
    }
    // A full pass that cannot start keeps its mode for the retry.
    let not_started = || {
        if mode == RuleSetRefreshMode::All {
            FULL.store(true, Ordering::SeqCst);
        }
        false
    };
    let Some(urls) = required_urls(state) else {
        return not_started();
    };
    let cache_dir = state.paths.rule_set_cache_dir();
    ice_engine::prune_rule_set_cache(&cache_dir, &urls, ice_engine::RULE_SET_CACHE_MAX_IDLE);
    if urls.is_empty() {
        // No set is needed any more (the profile changed and was applied).
        APPLY_PENDING.store(false, Ordering::SeqCst);
        return true;
    }
    let Ok(mgr) = application::subscription_manager(state) else {
        return not_started();
    };
    // Network phase without the orchestrate lock.
    let reports = mgr.refresh_rule_sets(&cache_dir, &urls, mode);
    let changed = reports.iter().filter(|r| r.changed()).count();
    let failed = reports
        .iter()
        .filter(|r| matches!(r.outcome, ice_engine::RuleSetRefresh::Failed(_)))
        .count();
    tracing::info!(
        ?mode,
        total = reports.len(),
        changed,
        failed,
        "rule-set refresh"
    );
    if changed > 0 {
        APPLY_PENDING.store(true, Ordering::SeqCst);
    }
    if !APPLY_PENDING.load(Ordering::SeqCst) {
        return true;
    }
    let Ok(_orch) = state.orchestrate.try_lock() else {
        tracing::debug!("rule-set refresh: orchestrate busy, re-applying later");
        return false;
    };
    if state.workers.is_paused_or_stopping() {
        return true;
    }
    // Cleared before Apply reads the cache, so content downloaded meanwhile
    // sets it again and is applied by a later pass.
    APPLY_PENDING.store(false, Ordering::SeqCst);
    let settings = current_settings(&state.paths).unwrap_or_default();
    if let Some(warning) = application::apply_after_subscription_change(app, state, &settings) {
        tracing::warn!(code = %warning.code, error = %warning.message, "rule-set refresh: apply warning");
    }
    drop(_orch);
    application::broadcast_state_change(app);
    true
}

pub fn spawn_rule_set_watchdog(app: AppHandle) {
    let workers = app.state::<AppState>().workers.clone();
    workers.spawn("rule-set-refresh", move |cancel| {
        if !cancel.wait(STARTUP_GRACE) {
            return;
        }
        // Run the first pass right away.
        let mut next_tick = Instant::now();
        // A pass whose Apply was deferred re-runs as a missing-only pass, so
        // nothing is fetched again.
        let mut retry = false;
        loop {
            let requested = REQUESTED.swap(false, Ordering::SeqCst);
            if requested || retry || Instant::now() >= next_tick {
                let mode = if FULL.swap(false, Ordering::SeqCst) {
                    RuleSetRefreshMode::All
                } else {
                    RuleSetRefreshMode::Missing
                };
                let Some(state) = app.try_state::<AppState>() else {
                    return;
                };
                retry = !refresh_pass(state.inner(), &app, mode);
                next_tick = Instant::now() + TICK;
            }
            if !cancel.wait(if retry { Duration::from_secs(15) } else { POLL }) {
                return;
            }
        }
    });
}
