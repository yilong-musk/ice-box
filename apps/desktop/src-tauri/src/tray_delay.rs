// SPDX-License-Identifier: GPL-3.0-or-later

//! Tray delay test: probe the exit nodes behind a node page.
//!
//! macOS-only feature. The module is compiled for the macOS target and — under
//! `cfg(test)` — on every host, so the platform-neutral model logic keeps
//! host-side unit coverage; the Windows and Linux release builds do not include
//! it and their tray keeps its old shape (see the `mod` declaration in
//! `lib.rs`).
//!
//! One button per page probes the page's *real exit nodes*, in page order: a
//! real node is probed itself, a strategy group follows its live `now` member
//! down to the leaf (a group without one is skipped), and an exit node behind
//! several rows is probed once. Probes run one at a time; the menu is
//! re-derived at most once a second while they do, and once more when the run
//! ends.
//!
//! A run is cancelled by closing the tray menu. The click that starts a test
//! closes the menu too, so the cancel is armed only once the menu has been seen
//! open again — `NSMenuDidBeginTrackingNotification` — and fires on the next
//! close (`NSMenuDidEndTrackingNotification`), between two probes.

use crate::commands::NodeInfo;
use std::collections::{BTreeMap, HashSet};
use std::sync::Mutex;

#[cfg(target_os = "macos")]
use crate::commands::{collect_nodes, probe_node_delay};
#[cfg(target_os = "macos")]
use crate::AppState;
#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "macos")]
use std::time::{Duration, Instant};
#[cfg(target_os = "macos")]
use tauri::{AppHandle, Manager};

/// Which node page asked for a test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DelayScope {
    /// The top page: every strategy group's current exit (a flat profile lists
    /// real nodes there, so it probes them all).
    Top,
    /// One strategy group's page: every member of that group.
    Group(String),
}

impl DelayScope {
    /// Menu id carrying the scope, in the same JSON-array form as the node
    /// actions, so tags holding any separator stay unambiguous.
    pub(crate) fn menu_id(&self) -> String {
        match self {
            Self::Top => serde_json::json!(["delay", "top"]).to_string(),
            Self::Group(tag) => serde_json::json!(["delay", "group", tag]).to_string(),
        }
    }

    /// Decode an id built by [`Self::menu_id`]. `None` for every other menu id
    /// (service switch, mode group, node items, subscription items, …).
    pub(crate) fn from_menu_id(id: &str) -> Option<Self> {
        if !id.starts_with('[') {
            return None;
        }
        let parts: Vec<String> = serde_json::from_str(id).ok()?;
        match parts.as_slice() {
            [kind, page] if kind == "delay" && page == "top" => Some(Self::Top),
            [kind, page, tag] if kind == "delay" && page == "group" => {
                Some(Self::Group(tag.clone()))
            }
            _ => None,
        }
    }
}

/// Outcome of one probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DelayOutcome {
    /// Queued or in flight; the menu prints `…`.
    Testing,
    /// Finished: the measured delay in milliseconds.
    Done(u32),
    /// The probe failed (core not running, API error, timeout).
    Failed,
}

/// The newest test as the menu needs it: the run in flight, and the outcome of
/// every tag a test has touched.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DelayView {
    /// Scope, 1-based index of the probe in flight, and total, while a test
    /// runs; `None` when idle.
    pub(crate) run: Option<(DelayScope, usize, usize)>,
    /// Outcome per probed tag.
    pub(crate) outcomes: BTreeMap<String, DelayOutcome>,
}

impl DelayView {
    /// Outcome recorded for `tag`, if a test touched it.
    pub(crate) fn outcome(&self, tag: &str) -> Option<DelayOutcome> {
        self.outcomes.get(tag).copied()
    }

    /// Progress of `scope`'s own run, while that page's test is the one in
    /// flight.
    pub(crate) fn progress(&self, scope: &DelayScope) -> Option<(usize, usize)> {
        self.run
            .as_ref()
            .filter(|(running, _, _)| running == scope)
            .map(|(_, index, total)| (*index, *total))
    }

    /// Whether the page buttons accept a click. Every button is disabled while
    /// any test runs, like the Nodes page disables its test buttons.
    pub(crate) fn idle(&self) -> bool {
        self.run.is_none()
    }
}

/// The run in flight.
#[derive(Debug)]
struct RunState {
    scope: DelayScope,
    tags: Vec<String>,
    /// Probes finished so far; the tag at this index is the one in flight.
    next: usize,
}

#[derive(Debug, Default)]
struct DelayState {
    outcomes: BTreeMap<String, DelayOutcome>,
    run: Option<RunState>,
}

impl DelayState {
    fn view(&self) -> DelayView {
        let run = self.run.as_ref().map(|run| {
            (
                run.scope.clone(),
                (run.next + 1).min(run.tags.len()),
                run.tags.len(),
            )
        });
        DelayView {
            run,
            outcomes: self.outcomes.clone(),
        }
    }

    /// Start a run over `tags`, marking every one of them `Testing` so a
    /// reopened menu shows what is still to come. Results of tags the run does
    /// not touch stay, like the results table on the Nodes page.
    fn begin(&mut self, scope: DelayScope, tags: Vec<String>) {
        for tag in &tags {
            self.outcomes.insert(tag.clone(), DelayOutcome::Testing);
        }
        self.run = Some(RunState {
            scope,
            tags,
            next: 0,
        });
    }

    /// Record the probe of `tags[index]` and move the progress on.
    fn advance(&mut self, index: usize, tag: &str, outcome: DelayOutcome) {
        self.outcomes.insert(tag.to_string(), outcome);
        if let Some(run) = self.run.as_mut() {
            run.next = index + 1;
        }
    }

    /// End the run. A cancelled run drops the `Testing` markers of the probes
    /// it never reached: those tags were not tested, so they show no result.
    fn finish(&mut self, cancelled: bool) {
        let Some(run) = self.run.take() else {
            return;
        };
        if cancelled {
            for tag in run.tags.iter().skip(run.next) {
                self.outcomes.remove(tag);
            }
        }
    }
}

/// The newest test. Written by the runner, read by the menu sync.
static STATE: Mutex<DelayState> = Mutex::new(DelayState {
    outcomes: BTreeMap::new(),
    run: None,
});

fn lock_state() -> std::sync::MutexGuard<'static, DelayState> {
    STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The newest test as the menu should draw it.
pub(crate) fn current_view() -> DelayView {
    lock_state().view()
}

/// The real outbound `tag` exits through right now: a leaf node is itself, a
/// strategy group follows its live `now` member (nested groups recurse). `None`
/// when the group has no current member or the chain leaves the known list.
pub(crate) fn resolve_exit_tag(nodes: &[NodeInfo], tag: &str) -> Option<String> {
    let mut seen = HashSet::new();
    resolve_exit_tag_inner(nodes, tag, &mut seen)
}

fn resolve_exit_tag_inner(
    nodes: &[NodeInfo],
    tag: &str,
    seen: &mut HashSet<String>,
) -> Option<String> {
    // An empty tag or a `now` chain that loops back on itself has no exit to
    // probe; the guard also keeps a malformed profile from hanging the runner.
    if tag.is_empty() || !seen.insert(tag.to_string()) {
        return None;
    }
    let node = nodes.iter().find(|node| node.tag == tag)?;
    if node.group_all.is_none() {
        return Some(tag.to_string());
    }
    let now = node.group_now.as_deref().filter(|now| !now.is_empty())?;
    resolve_exit_tag_inner(nodes, now, seen)
}

/// The exit nodes a page's button probes, in page order, deduplicated.
pub(crate) fn delay_test_tags(nodes: &[NodeInfo], scope: &DelayScope) -> Vec<String> {
    let has_groups = nodes.iter().any(|node| node.group_all.is_some());
    let rows: Vec<&str> = match scope {
        // The top page lists groups when the profile has them (a group with no
        // members is not on the page) and every node otherwise.
        DelayScope::Top if has_groups => nodes
            .iter()
            .filter(|node| node.group_all.as_ref().is_some_and(|all| !all.is_empty()))
            .map(|node| node.tag.as_str())
            .collect(),
        DelayScope::Top => nodes.iter().map(|node| node.tag.as_str()).collect(),
        DelayScope::Group(tag) => nodes
            .iter()
            .find(|node| node.tag == *tag)
            .and_then(|node| node.group_all.as_deref())
            .unwrap_or_default()
            .iter()
            .map(String::as_str)
            .collect(),
    };
    let mut seen = HashSet::new();
    let mut tags = Vec::new();
    for row in rows {
        let Some(exit) = resolve_exit_tag(nodes, row) else {
            continue;
        };
        if seen.insert(exit.clone()) {
            tags.push(exit);
        }
    }
    tags
}

/// Menu refresh throttle while a test runs: each rebuild re-derives the whole
/// menu, and results land one probe at a time, so once a second is plenty.
#[cfg(target_os = "macos")]
const REFRESH_INTERVAL: Duration = Duration::from_secs(1);

/// Set while a test runs: a second click is ignored, and a menu close cancels.
#[cfg(target_os = "macos")]
static RUN_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Set when the tray menu opens. Reset when a test starts, so the close that
/// follows the start click — the menu was already open when it was clicked — is
/// not read as a cancel.
#[cfg(target_os = "macos")]
static MENU_OPENED: AtomicBool = AtomicBool::new(false);

/// Set by a close of the tray menu after a test start; consumed between probes.
#[cfg(target_os = "macos")]
static CANCEL_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Tray「测速」: probe the page's exit nodes in order and refresh the menu as
/// results land. Off the main thread like the other tray actions.
#[cfg(target_os = "macos")]
pub(crate) fn start(app: &AppHandle, scope: DelayScope) {
    // Claim the run before spawning: two clicks must not queue two tests.
    if RUN_ACTIVE.swap(true, Ordering::SeqCst) {
        return;
    }
    CANCEL_REQUESTED.store(false, Ordering::SeqCst);
    // The menu is open right now (the click came from it); the close that
    // follows this call is the start click's own and must not cancel the test.
    MENU_OPENED.store(false, Ordering::SeqCst);
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || run(app, scope));
}

#[cfg(target_os = "macos")]
fn run(app: AppHandle, scope: DelayScope) {
    let Some(state) = app.try_state::<AppState>() else {
        RUN_ACTIVE.store(false, Ordering::SeqCst);
        return;
    };
    let tags = match collect_nodes(state.inner()) {
        Ok(nodes) => delay_test_tags(&nodes, &scope),
        Err(err) => {
            tracing::warn!(
                code = %err.code,
                error = %err.message,
                "tray delay target derivation failed"
            );
            Vec::new()
        }
    };
    if tags.is_empty() {
        RUN_ACTIVE.store(false, Ordering::SeqCst);
        return;
    }
    lock_state().begin(scope, tags.clone());
    // Show `…` and the progress right away, so a menu reopened during the run
    // is not still showing the idle labels.
    crate::tray::sync_menu(&app);
    let mut last_refresh = Instant::now();
    for (index, tag) in tags.iter().enumerate() {
        // A close of the menu cancels between probes; the probe in flight runs
        // to its own timeout (5s) first.
        if CANCEL_REQUESTED.load(Ordering::SeqCst) {
            break;
        }
        let outcome = match probe_node_delay(state.inner(), tag) {
            Ok(delay_ms) => DelayOutcome::Done(delay_ms),
            Err(err) => {
                tracing::warn!(
                    code = %err.code,
                    error = %err.message,
                    tag = %tag,
                    "tray delay probe failed"
                );
                DelayOutcome::Failed
            }
        };
        lock_state().advance(index, tag, outcome);
        if last_refresh.elapsed() >= REFRESH_INTERVAL {
            crate::tray::sync_menu(&app);
            last_refresh = Instant::now();
        }
    }
    lock_state().finish(CANCEL_REQUESTED.load(Ordering::SeqCst));
    RUN_ACTIVE.store(false, Ordering::SeqCst);
    MENU_OPENED.store(false, Ordering::SeqCst);
    CANCEL_REQUESTED.store(false, Ordering::SeqCst);
    // Final refresh, always: the buttons must go back to their labels and the
    // last results must reach the menu.
    crate::tray::sync_menu(&app);
}

/// Observe the tray menu's tracking: remember that it opened, and cancel the
/// test in flight when it closes again.
///
/// AppKit posts `NSMenuDid{Begin,End}TrackingNotification` on the default
/// notification center with the menu as the notification object, so an
/// observer registered with `object = tray menu` sees this menu only and the
/// app's other menus (the window's menu bar) keep their tracking to
/// themselves.
#[cfg(target_os = "macos")]
pub(crate) fn install_menu_watch(tray: &tauri::tray::TrayIcon<tauri::Wry>) {
    let registered = tray.with_inner_tray_icon(|tray| {
        let Some(mtm) = objc2::MainThreadMarker::new() else {
            return false;
        };
        let Some(status_item) = tray.ns_status_item() else {
            return false;
        };
        let Some(menu) = status_item.menu(mtm) else {
            return false;
        };
        let center = objc2_foundation::NSNotificationCenter::defaultCenter();
        // The observer's `object` filter: this menu, and no other.
        let object: &objc2::runtime::AnyObject = &menu;
        // SAFETY: AppKit declares the notification names as constant statics;
        // reading them neither mutates nor races with anything.
        let (begin, end) = unsafe {
            (
                objc2_app_kit::NSMenuDidBeginTrackingNotification,
                objc2_app_kit::NSMenuDidEndTrackingNotification,
            )
        };
        for (name, handler) in [
            (begin, on_menu_opened as fn()),
            (end, on_menu_closed as fn()),
        ] {
            let block: block2::RcBlock<
                dyn Fn(std::ptr::NonNull<objc2_foundation::NSNotification>),
            > = block2::RcBlock::new(
                move |_notification: std::ptr::NonNull<objc2_foundation::NSNotification>| handler(),
            );
            // SAFETY: `name` is AppKit's own constant, `object` is the tray
            // menu the notification is posted for, and `None` for the queue
            // delivers the block on the posting thread — the main thread, where
            // menu tracking runs, so the handler only touches atomics. The
            // block is copied by the center and stays valid for the process.
            unsafe {
                let observer = center.addObserverForName_object_queue_usingBlock(
                    Some(name),
                    Some(object),
                    None,
                    &block,
                );
                // Process-lifetime observer: leaking the token is what keeps
                // the registration alive (removing it would unregister the
                // only thing that arms the cancel).
                std::mem::forget(observer);
            }
        }
        true
    });
    match registered {
        Ok(true) => {}
        Ok(false) => tracing::warn!("tray delay: tray menu not found; close-to-cancel is off"),
        Err(err) => tracing::warn!(error = %err, "tray delay: tray menu watch failed"),
    }
}

#[cfg(target_os = "macos")]
fn on_menu_opened() {
    MENU_OPENED.store(true, Ordering::SeqCst);
}

#[cfg(target_os = "macos")]
fn on_menu_closed() {
    if RUN_ACTIVE.load(Ordering::SeqCst) && MENU_OPENED.load(Ordering::SeqCst) {
        CANCEL_REQUESTED.store(true, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(tag: &str, now: Option<&str>, all: Option<&[&str]>) -> NodeInfo {
        NodeInfo {
            tag: tag.to_string(),
            outbound_type: if all.is_some() { "selector" } else { "socks" }.to_string(),
            group_now: now.map(str::to_string),
            group_all: all.map(|members| members.iter().map(|m| m.to_string()).collect()),
        }
    }

    fn grouped() -> Vec<NodeInfo> {
        vec![
            node("节点选择", Some("日本 02"), Some(&["香港 01", "日本 02"])),
            node("自动选择", Some("香港 01"), Some(&["香港 01", "日本 02"])),
            node("香港 01", None, None),
            node("日本 02", None, None),
        ]
    }

    #[test]
    fn top_scope_probes_every_node_of_a_flat_profile() {
        let nodes = vec![node("香港 01", None, None), node("日本 02", None, None)];
        assert_eq!(
            delay_test_tags(&nodes, &DelayScope::Top),
            vec!["香港 01", "日本 02"]
        );
    }

    #[test]
    fn top_scope_probes_each_groups_current_exit_once() {
        let mut nodes = grouped();
        // Both groups now exit through 日本 02: one probe, not two.
        nodes[1].group_now = Some("日本 02".to_string());
        assert_eq!(delay_test_tags(&nodes, &DelayScope::Top), vec!["日本 02"]);
    }

    #[test]
    fn top_scope_follows_nested_groups_to_the_leaf() {
        let nodes = vec![
            node("外层", Some("内层"), Some(&["内层"])),
            node("内层", Some("香港 01"), Some(&["香港 01"])),
            node("香港 01", None, None),
        ];
        assert_eq!(delay_test_tags(&nodes, &DelayScope::Top), vec!["香港 01"]);
    }

    #[test]
    fn group_scope_probes_the_members_in_page_order() {
        let nodes = vec![
            node(
                "节点选择",
                Some("日本 02"),
                Some(&["香港 01", "日本 02", "香港 01"]),
            ),
            node("香港 01", None, None),
            node("日本 02", None, None),
        ];
        assert_eq!(
            delay_test_tags(&nodes, &DelayScope::Group("节点选择".to_string())),
            vec!["香港 01", "日本 02"]
        );
    }

    #[test]
    fn group_scope_follows_a_nested_member_to_the_leaf() {
        let nodes = vec![
            node("外层", None, Some(&["内层"])),
            node("内层", Some("香港 01"), Some(&["香港 01"])),
            node("香港 01", None, None),
        ];
        assert_eq!(
            delay_test_tags(&nodes, &DelayScope::Group("外层".to_string())),
            vec!["香港 01"]
        );
        assert_eq!(
            delay_test_tags(&nodes, &DelayScope::Group("不存在".to_string())),
            Vec::<String>::new()
        );
    }

    #[test]
    fn the_top_page_skips_groups_without_a_current_exit() {
        let nodes = vec![
            node("空组", None, Some(&[])),
            node("无出口", None, Some(&["香港 01"])),
            node("香港 01", None, None),
        ];
        // The empty group is not on the page, and a group with no current exit
        // has nothing to probe.
        assert_eq!(
            delay_test_tags(&nodes, &DelayScope::Top),
            Vec::<String>::new()
        );
        // Its own page probes its members all the same: the page lists them.
        assert_eq!(
            delay_test_tags(&nodes, &DelayScope::Group("无出口".to_string())),
            vec!["香港 01"]
        );
    }

    #[test]
    fn a_now_chain_that_loops_back_probes_nothing() {
        let nodes = vec![
            node("A", Some("B"), Some(&["B"])),
            node("B", Some("A"), Some(&["A"])),
        ];
        assert_eq!(
            delay_test_tags(&nodes, &DelayScope::Top),
            Vec::<String>::new()
        );
    }

    #[test]
    fn scope_ids_round_trip_for_awkward_tags() {
        for scope in [
            DelayScope::Top,
            DelayScope::Group("组:1 | A&B [2]".to_string()),
        ] {
            assert_eq!(DelayScope::from_menu_id(&scope.menu_id()), Some(scope));
        }
        // Every other id space stays its own.
        for id in [
            "service",
            "show",
            "[\"node\",\"香港 01\"]",
            "[\"sub\",\"11111111-1111-4111-8111-111111111111\"]",
            "[\"delay\"]",
            "[\"delay\",\"group\"]",
            "[\"delay\",\"top\",\"extra\"]",
        ] {
            assert_eq!(DelayScope::from_menu_id(id), None, "{id}");
        }
    }

    #[test]
    fn a_run_marks_every_tag_then_records_the_outcomes() {
        let mut state = DelayState::default();
        state.begin(DelayScope::Top, vec!["A".to_string(), "B".to_string()]);
        let view = state.view();
        assert_eq!(view.run, Some((DelayScope::Top, 1, 2)));
        assert_eq!(view.outcome("A"), Some(DelayOutcome::Testing));
        assert_eq!(view.outcome("B"), Some(DelayOutcome::Testing));
        assert!(!view.idle());

        state.advance(0, "A", DelayOutcome::Done(45));
        let view = state.view();
        assert_eq!(view.run, Some((DelayScope::Top, 2, 2)));
        assert_eq!(view.outcome("A"), Some(DelayOutcome::Done(45)));
        assert_eq!(view.outcome("B"), Some(DelayOutcome::Testing));

        state.advance(1, "B", DelayOutcome::Failed);
        state.finish(false);
        let view = state.view();
        assert!(view.idle());
        assert_eq!(view.progress(&DelayScope::Top), None);
        assert_eq!(view.outcome("A"), Some(DelayOutcome::Done(45)));
        assert_eq!(view.outcome("B"), Some(DelayOutcome::Failed));
    }

    #[test]
    fn a_cancelled_run_drops_the_probes_it_never_reached() {
        let mut state = DelayState::default();
        state.begin(
            DelayScope::Group("G".to_string()),
            vec!["A".to_string(), "B".to_string(), "C".to_string()],
        );
        state.advance(0, "A", DelayOutcome::Done(30));
        state.finish(true);
        let view = state.view();
        assert!(view.idle());
        assert_eq!(view.outcome("A"), Some(DelayOutcome::Done(30)));
        assert_eq!(view.outcome("B"), None);
        assert_eq!(view.outcome("C"), None);
    }

    #[test]
    fn progress_only_names_the_running_page() {
        let mut state = DelayState::default();
        state.begin(
            DelayScope::Group("G".to_string()),
            vec!["A".to_string(), "B".to_string()],
        );
        let view = state.view();
        assert_eq!(
            view.progress(&DelayScope::Group("G".to_string())),
            Some((1, 2))
        );
        assert_eq!(view.progress(&DelayScope::Top), None);
        assert_eq!(view.progress(&DelayScope::Group("other".to_string())), None);
    }

    #[test]
    fn a_new_run_keeps_the_results_of_tags_it_does_not_probe() {
        let mut state = DelayState::default();
        state.begin(DelayScope::Top, vec!["A".to_string()]);
        state.advance(0, "A", DelayOutcome::Done(10));
        state.finish(false);
        state.begin(DelayScope::Group("G".to_string()), vec!["B".to_string()]);
        let view = state.view();
        assert_eq!(view.outcome("A"), Some(DelayOutcome::Done(10)));
        assert_eq!(view.outcome("B"), Some(DelayOutcome::Testing));
    }
}
