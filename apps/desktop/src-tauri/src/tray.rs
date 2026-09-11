// SPDX-License-Identifier: GPL-3.0-or-later

//! System tray: close → hide; left click → show; Quit → Stop then exit.
//!
//! The menu also carries the actions that do not need the window: the proxy
//! service switch (labeled with the action, like the Home power button), the
//! routing mode group, and the node groups. A watchdog re-derives all of them
//! from the runtime state, so the menu follows changes made anywhere else —
//! window, recovery, or a manual OS edit.

use crate::capture::TrafficCapture;
use crate::commands::{
    apply_proxy_mode, broadcast_state_change, collect_nodes, current_settings,
    disable_active_backend_inner, lock_orchestrate, proxy_service_posture, select_group_member,
    select_node, start_service, NodeInfo,
};
use crate::shutdown::{request_tray_quit, QuitOutcome};
use crate::AppState;
use ice_config::{AppError, ErrorCode, LanguagePreference, ProxyMode};
use ice_core::CoreStatus;
use serde::Deserialize;
use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{
    menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, Wry,
};

/// Menu re-derivation cadence. Longer than the 2s status poll on purpose: the
/// live OS-proxy probe spawns `networksetup` subprocesses on macOS, and the
/// menu tolerates a few seconds of lag (it is re-derived right after every tray
/// action anyway). The `proxy_applied_cache` is shared with the poll, so while
/// the window is open the probe is usually a cache hit.
const SYNC_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrayLanguage {
    Zh,
    En,
}

impl TrayLanguage {
    fn code(self) -> u8 {
        match self {
            Self::Zh => 0,
            Self::En => 1,
        }
    }

    fn from_code(code: u8) -> Self {
        if code == 1 {
            Self::En
        } else {
            Self::Zh
        }
    }
}

impl From<LanguagePreference> for TrayLanguage {
    fn from(preference: LanguagePreference) -> Self {
        match preference {
            LanguagePreference::Zh => Self::Zh,
            LanguagePreference::System | LanguagePreference::En => Self::En,
        }
    }
}

fn mode_code(mode: ProxyMode) -> u8 {
    match mode {
        ProxyMode::Rule => 0,
        ProxyMode::Global => 1,
        ProxyMode::Direct => 2,
    }
}

fn mode_from_code(code: u8) -> ProxyMode {
    match code {
        1 => ProxyMode::Global,
        2 => ProxyMode::Direct,
        _ => ProxyMode::Rule,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TrayLabels {
    service_start: &'static str,
    service_stop: &'static str,
    mode: &'static str,
    mode_rule: &'static str,
    mode_global: &'static str,
    mode_direct: &'static str,
    nodes: &'static str,
    show: &'static str,
    quit: &'static str,
}

impl TrayLabels {
    /// The switch names the action it performs, like the Home power button.
    fn service(self, engaged: bool) -> &'static str {
        if engaged {
            self.service_stop
        } else {
            self.service_start
        }
    }
}

fn labels(language: TrayLanguage) -> TrayLabels {
    match language {
        TrayLanguage::Zh => TrayLabels {
            service_start: "启动代理服务",
            service_stop: "停止代理服务",
            mode: "代理模式",
            mode_rule: "规则",
            mode_global: "全局",
            mode_direct: "直连",
            nodes: "节点",
            show: "显示",
            quit: "退出",
        },
        TrayLanguage::En => TrayLabels {
            service_start: "Start Proxy Service",
            service_stop: "Stop Proxy Service",
            mode: "Proxy Mode",
            mode_rule: "Rule",
            mode_global: "Global",
            mode_direct: "Direct",
            nodes: "Nodes",
            show: "Show",
            quit: "Quit",
        },
    }
}

/// What a node menu item does when clicked.
#[derive(Debug, Clone, PartialEq, Eq)]
enum NodeAction {
    /// Flat profile (no strategy groups): pick the tag of the injected `proxy`
    /// selector, the same call the Nodes page 「选择」 makes.
    SelectNode(String),
    /// Strategy-group member: switch the member `group` points at, the same call
    /// the Nodes page makes when a member row is clicked.
    SelectMember { group: String, member: String },
}

impl NodeAction {
    /// Menu id carrying the action. Encoded as a JSON array so tags holding any
    /// separator stay unambiguous, and so the click path needs no shared map to
    /// look the action up (it must never block the main thread).
    fn menu_id(&self) -> String {
        match self {
            Self::SelectNode(tag) => serde_json::json!(["node", tag]).to_string(),
            Self::SelectMember { group, member } => {
                serde_json::json!(["member", group, member]).to_string()
            }
        }
    }
}

/// Decode an id built by [`NodeAction::menu_id`]. `None` for every other menu id
/// (service switch, mode group, submenu parents, …).
fn node_action_from_menu_id(id: &str) -> Option<NodeAction> {
    if !id.starts_with('[') {
        return None;
    }
    let parts: Vec<String> = serde_json::from_str(id).ok()?;
    match parts.as_slice() {
        [kind, tag] if kind == "node" => Some(NodeAction::SelectNode(tag.clone())),
        [kind, group, member] if kind == "member" => Some(NodeAction::SelectMember {
            group: group.clone(),
            member: member.clone(),
        }),
        _ => None,
    }
}

/// One check item of the node submenu.
#[derive(Debug, Clone, PartialEq, Eq)]
struct NodeMenuItem {
    label: String,
    enabled: bool,
    checked: bool,
    action: NodeAction,
}

/// A strategy group: its members sit one level down.
#[derive(Debug, Clone, PartialEq, Eq)]
struct NodeMenuGroup {
    label: String,
    members: Vec<NodeMenuItem>,
}

/// Node submenu body. Derived from the active profile and the live selections;
/// the submenu is rebuilt only when this value changes.
#[derive(Debug, Clone, PartialEq, Eq)]
enum NodeMenuEntry {
    Item(NodeMenuItem),
    Group(NodeMenuGroup),
}

/// Group types whose members can be switched. Mirrors the Nodes page: the other
/// strategy types pick their member themselves, so theirs are listed read-only.
const SELECTABLE_GROUP_TYPE: &str = "selector";

/// Derive the node submenu body.
///
/// Flat profiles (v1 fallback: no strategy groups) list every node, because a
/// pick lands in the injected `proxy` selector. Grouped profiles list their
/// groups with the members one level down — a node outside every group has no
/// group to be switched in (and grouped profiles have no flat selector), so it
/// is not offered.
fn node_menu_entries(nodes: &[NodeInfo], selected: &str) -> Vec<NodeMenuEntry> {
    let groups: Vec<&NodeInfo> = nodes
        .iter()
        .filter(|node| node.group_all.is_some())
        .collect();
    if groups.is_empty() {
        return nodes
            .iter()
            .map(|node| {
                NodeMenuEntry::Item(NodeMenuItem {
                    label: node.tag.clone(),
                    enabled: true,
                    checked: node.tag == selected,
                    action: NodeAction::SelectNode(node.tag.clone()),
                })
            })
            .collect();
    }
    groups
        .iter()
        .filter_map(|group| {
            let members = group.group_all.as_deref()?;
            if members.is_empty() {
                return None;
            }
            let selectable = group.outbound_type == SELECTABLE_GROUP_TYPE;
            let now = group.group_now.as_deref().filter(|now| !now.is_empty());
            Some(NodeMenuEntry::Group(NodeMenuGroup {
                label: group_label(&group.tag, now),
                members: members
                    .iter()
                    .map(|member| NodeMenuItem {
                        label: member.clone(),
                        enabled: selectable,
                        checked: now == Some(member.as_str()),
                        action: NodeAction::SelectMember {
                            group: group.tag.clone(),
                            member: member.clone(),
                        },
                    })
                    .collect(),
            }))
        })
        .collect()
}

/// `tag → current member`: the collapsed submenu then shows the live exit too.
fn group_label(tag: &str, now: Option<&str>) -> String {
    match now {
        Some(now) => format!("{tag} → {now}"),
        None => tag.to_string(),
    }
}

/// muda reads `&` as a mnemonic marker on Windows and strips a lone `&` on
/// macOS; doubling keeps subscription tags containing `&` literal. GTK uses the
/// text as-is.
fn menu_text(text: &str) -> Cow<'_, str> {
    if cfg!(any(target_os = "windows", target_os = "macos")) && text.contains('&') {
        Cow::Owned(text.replace('&', "&&"))
    } else {
        Cow::Borrowed(text)
    }
}

/// Tray menu plumbing failures all surface as `ConfigInvalid`; the message keeps
/// the failing step identifiable in logs.
fn tray_error(step: &str, err: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::ConfigInvalid, format!("{step}: {err}"))
}

/// What the menu shows. Derived from the same values the Home page renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TrayView {
    service_on: bool,
    /// Whether this platform can start capture at all (`proxyAvailable` /
    /// `tunAvailable` in the Home page); the switch is disabled otherwise.
    service_enabled: bool,
    mode: ProxyMode,
}

impl Default for TrayView {
    fn default() -> Self {
        Self {
            service_on: false,
            // A failed read must not disable the switch; the next sync decides.
            service_enabled: true,
            mode: ProxyMode::Rule,
        }
    }
}

struct TrayMenuState {
    service: MenuItem<Wry>,
    mode: Submenu<Wry>,
    mode_rule: CheckMenuItem<Wry>,
    mode_global: CheckMenuItem<Wry>,
    mode_direct: CheckMenuItem<Wry>,
    nodes: Submenu<Wry>,
    /// Body currently attached to `nodes`. Locked only off the main thread: a
    /// rebuild blocks on main-thread menu mutations, so holding this lock on the
    /// main thread while the watchdog rebuilds would deadlock both.
    nodes_model: Mutex<Vec<NodeMenuEntry>>,
    /// Set by a node click, which needs a rebuild even when the derived model is
    /// unchanged: the platform toggles the clicked check item itself, so a
    /// re-click or a failed switch would leave a wrong check mark on screen.
    /// Written from the menu thread (no lock), consumed by the sync.
    nodes_dirty: AtomicBool,
    show: MenuItem<Wry>,
    quit: MenuItem<Wry>,
    language: AtomicU8,
    service_on: AtomicBool,
    service_enabled: AtomicBool,
    mode_value: AtomicU8,
}

impl TrayMenuState {
    fn labels(&self) -> TrayLabels {
        labels(TrayLanguage::from_code(
            self.language.load(Ordering::SeqCst),
        ))
    }

    fn service_on(&self) -> bool {
        self.service_on.load(Ordering::SeqCst)
    }

    fn service_enabled(&self) -> bool {
        self.service_enabled.load(Ordering::SeqCst)
    }

    fn mode(&self) -> ProxyMode {
        mode_from_code(self.mode_value.load(Ordering::SeqCst))
    }

    fn update_label(&self, what: &str, result: tauri::Result<()>) -> Result<(), AppError> {
        result.map_err(|err| tray_error(&format!("update tray {what} label"), err))
    }

    /// Rewrite every label for `language` and remember the choice.
    fn apply_language(&self, language: TrayLanguage) -> Result<(), AppError> {
        let labels = labels(language);
        self.update_label(
            "service",
            self.service.set_text(labels.service(self.service_on())),
        )?;
        self.update_label("mode", self.mode.set_text(labels.mode))?;
        self.update_label("rule mode", self.mode_rule.set_text(labels.mode_rule))?;
        self.update_label("global mode", self.mode_global.set_text(labels.mode_global))?;
        self.update_label("direct mode", self.mode_direct.set_text(labels.mode_direct))?;
        self.update_label("nodes", self.nodes.set_text(labels.nodes))?;
        self.update_label("Show", self.show.set_text(labels.show))?;
        self.update_label("Quit", self.quit.set_text(labels.quit))?;
        self.language.store(language.code(), Ordering::SeqCst);
        Ok(())
    }

    /// Attach the derived node body, rebuilding the submenu only when the model
    /// changed. The submenu handle is stable, so the language refresh can keep
    /// rewriting its title without touching this path.
    fn apply_nodes(&self, app: &AppHandle, entries: &[NodeMenuEntry]) -> Result<(), AppError> {
        let dirty = self.nodes_dirty.load(Ordering::SeqCst);
        let mut applied = self
            .nodes_model
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !dirty && applied.as_slice() == entries {
            return Ok(());
        }
        clear_node_children(&self.nodes)?;
        if let Err(err) = append_node_entries(app, &self.nodes, entries) {
            // Leave the model as it was, so the next sync retries the rebuild.
            let _ = clear_node_children(&self.nodes);
            return Err(err);
        }
        self.nodes
            .set_enabled(!entries.is_empty())
            .map_err(|err| tray_error("update tray nodes enabled", err))?;
        *applied = entries.to_vec();
        self.nodes_dirty.store(false, Ordering::SeqCst);
        Ok(())
    }

    /// Re-assert the whole mode group. Needed after a click: the platform menu
    /// toggles the clicked check item itself, even when it was already active.
    fn apply_mode(&self, mode: ProxyMode) {
        let _ = self.mode_rule.set_checked(mode == ProxyMode::Rule);
        let _ = self.mode_global.set_checked(mode == ProxyMode::Global);
        let _ = self.mode_direct.set_checked(mode == ProxyMode::Direct);
        self.mode_value.store(mode_code(mode), Ordering::SeqCst);
    }

    /// Apply a derived view, touching only the items that changed.
    fn apply_view(&self, view: TrayView) {
        if self.service_enabled() != view.service_enabled {
            let _ = self.service.set_enabled(view.service_enabled);
            self.service_enabled
                .store(view.service_enabled, Ordering::SeqCst);
        }
        if self.service_on() != view.service_on {
            let _ = self
                .service
                .set_text(self.labels().service(view.service_on));
            self.service_on.store(view.service_on, Ordering::SeqCst);
        }
        if self.mode() != view.mode {
            self.apply_mode(view.mode);
        }
    }
}

/// Drop every child of the node submenu.
fn clear_node_children(parent: &Submenu<Wry>) -> Result<(), AppError> {
    while parent
        .remove_at(0)
        .map_err(|err| tray_error("clear tray nodes", err))?
        .is_some()
    {}
    Ok(())
}

/// Build and attach the node submenu body: check items at this level and one
/// submenu per strategy group.
fn append_node_entries(
    app: &AppHandle,
    parent: &Submenu<Wry>,
    entries: &[NodeMenuEntry],
) -> Result<(), AppError> {
    for (index, entry) in entries.iter().enumerate() {
        match entry {
            NodeMenuEntry::Item(item) => {
                parent
                    .append(&node_check_item(app, item)?)
                    .map_err(|err| tray_error("append tray node item", err))?;
            }
            NodeMenuEntry::Group(group) => {
                let members = group
                    .members
                    .iter()
                    .map(|member| node_check_item(app, member))
                    .collect::<Result<Vec<_>, AppError>>()?;
                let member_refs: Vec<&dyn IsMenuItem<Wry>> = members
                    .iter()
                    .map(|member| member as &dyn IsMenuItem<Wry>)
                    .collect();
                let submenu = Submenu::with_id_and_items(
                    app,
                    serde_json::json!(["group", index]).to_string(),
                    menu_text(&group.label),
                    true,
                    &member_refs,
                )
                .map_err(|err| tray_error("create tray node group", err))?;
                parent
                    .append(&submenu)
                    .map_err(|err| tray_error("append tray node group", err))?;
            }
        }
    }
    Ok(())
}

fn node_check_item(app: &AppHandle, item: &NodeMenuItem) -> Result<CheckMenuItem<Wry>, AppError> {
    CheckMenuItem::with_id(
        app,
        item.action.menu_id(),
        menu_text(&item.label),
        item.enabled,
        item.checked,
        None::<&str>,
    )
    .map_err(|err| tray_error("create tray node item", err))
}

fn show_main_window(app: &AppHandle) {
    let Some(win) = app.get_webview_window("main") else {
        return;
    };
    if matches!(win.is_minimized(), Ok(true)) {
        let _ = win.unminimize();
    }
    let _ = win.show();
    let _ = win.set_focus();
}

pub fn setup_tray(app: &AppHandle, language: TrayLanguage) -> tauri::Result<()> {
    let labels = labels(language);
    let view = current_view(app).unwrap_or_default();
    let service = MenuItem::with_id(
        app,
        "service",
        labels.service(view.service_on),
        view.service_enabled,
        None::<&str>,
    )?;
    let mode_rule = CheckMenuItem::with_id(
        app,
        "mode:rule",
        labels.mode_rule,
        true,
        view.mode == ProxyMode::Rule,
        None::<&str>,
    )?;
    let mode_global = CheckMenuItem::with_id(
        app,
        "mode:global",
        labels.mode_global,
        true,
        view.mode == ProxyMode::Global,
        None::<&str>,
    )?;
    let mode_direct = CheckMenuItem::with_id(
        app,
        "mode:direct",
        labels.mode_direct,
        true,
        view.mode == ProxyMode::Direct,
        None::<&str>,
    )?;
    let mode = Submenu::with_items(
        app,
        labels.mode,
        true,
        &[&mode_rule, &mode_global, &mode_direct],
    )?;
    // Seeded here instead of through `apply_nodes`: that path takes a lock the
    // main thread must never hold (`nodes_model`), and the watchdog only kicks
    // in after its first sleep. A failed build leaves the entry empty so the
    // next sync retries it.
    let node_entries = current_node_entries(app).unwrap_or_default();
    let nodes = Submenu::with_id(app, "nodes", labels.nodes, false)?;
    let node_model = if node_entries.is_empty() {
        Vec::new()
    } else {
        match append_node_entries(app, &nodes, &node_entries) {
            Ok(()) => {
                let _ = nodes.set_enabled(true);
                node_entries
            }
            Err(err) => {
                tracing::warn!(
                    code = %err.code,
                    error = %err.message,
                    "tray node menu setup failed"
                );
                let _ = clear_node_children(&nodes);
                Vec::new()
            }
        }
    };
    let show = MenuItem::with_id(app, "show", labels.show, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", labels.quit, true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&service, &mode, &nodes, &separator, &show, &quit])?;
    app.manage(TrayMenuState {
        service,
        mode,
        mode_rule,
        mode_global,
        mode_direct,
        nodes,
        nodes_model: Mutex::new(node_model),
        nodes_dirty: AtomicBool::new(false),
        show,
        quit,
        language: AtomicU8::new(language.code()),
        service_on: AtomicBool::new(view.service_on),
        service_enabled: AtomicBool::new(view.service_enabled),
        mode_value: AtomicU8::new(mode_code(view.mode)),
    });

    let mut builder = TrayIconBuilder::new()
        .menu(&menu)
        .tooltip("ice-box")
        // Windows pops the menu on left click by default, which competes with
        // the activate gesture: there the left click opens the window and only
        // the right click opens the menu.
        .show_menu_on_left_click(!cfg!(target_os = "windows"))
        .on_menu_event(|app, event| {
            let id = event.id.as_ref();
            if let Some(action) = node_action_from_menu_id(id) {
                switch_node(app, action);
                return;
            }
            match id {
                "service" => toggle_service(app),
                "mode:rule" => switch_mode(app, ProxyMode::Rule),
                "mode:global" => switch_mode(app, ProxyMode::Global),
                "mode:direct" => switch_mode(app, ProxyMode::Direct),
                "show" => show_main_window(app),
                "quit" => {
                    // The stop can take seconds with TUN active (teardown waits +
                    // core stop + `networksetup` restore); run it off the main
                    // thread so the window never freezes, and exit from the
                    // worker once the state is consistent.
                    let app = app.clone();
                    tauri::async_runtime::spawn_blocking(move || match request_tray_quit(&app) {
                        QuitOutcome::Stopped => app.exit(0),
                        QuitOutcome::ProxyRestoreFailed | QuitOutcome::StopFailed => {}
                        QuitOutcome::LockPoisoned => app.exit(1),
                    });
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    let _tray = builder.build(app)?;
    Ok(())
}

/// Derive the menu view from the runtime state. `None` while the app state or
/// the settings file is unavailable (first launch, teardown): callers keep the
/// previous view instead of guessing.
fn current_view(app: &AppHandle) -> Option<TrayView> {
    let state = app.try_state::<AppState>()?;
    let settings = current_settings(&state.paths).ok()?;
    let running = state.core_snapshot.load().state.status == CoreStatus::Running;
    let capture = state.capture.status(&settings);
    let tun_active = capture.traffic_capture == TrafficCapture::Tun;
    let posture = proxy_service_posture(state.inner(), Some(&settings), running);
    Some(TrayView {
        service_on: posture.engaged(tun_active),
        service_enabled: state.system_proxy_available || capture.tun_available,
        mode: settings.proxy_mode,
    })
}

/// Derive the node submenu body from the active profile and the live picks.
/// `None` while the app state, the settings file, or the profile is unavailable
/// (first launch, teardown): callers keep the previous menu.
fn current_node_entries(app: &AppHandle) -> Option<Vec<NodeMenuEntry>> {
    let state = app.try_state::<AppState>()?;
    let settings = current_settings(&state.paths).ok()?;
    let nodes = collect_nodes(state.inner()).ok()?;
    let selected = resolve_selected_tag(&nodes, settings.selected_tag.as_deref());
    Some(node_menu_entries(&nodes, &selected))
}

/// Mirrors the Nodes page: a `selected_tag` that is not in the list falls back
/// to the first node, the same default `build_runtime_json` bakes in.
fn resolve_selected_tag(nodes: &[NodeInfo], selected: Option<&str>) -> String {
    if let Some(tag) = selected {
        if nodes.iter().any(|node| node.tag == tag) {
            return tag.to_string();
        }
    }
    nodes
        .first()
        .map(|node| node.tag.clone())
        .unwrap_or_default()
}

/// Re-derive the menu from the runtime state. Cheap enough for the watchdog:
/// one settings read, one proxy record read, the memoized OS probe, and the
/// node list (a cached profile plus, while the core runs, one Clash API call).
///
/// Off-main-thread only: rebuilding the node submenu blocks on main-thread menu
/// mutations, so a watchdog rebuild plus a main-thread caller here would
/// deadlock on `nodes_model`.
pub fn sync_menu(app: &AppHandle) {
    let Some(menu) = app.try_state::<TrayMenuState>() else {
        return;
    };
    let Some(view) = current_view(app) else {
        return;
    };
    menu.apply_view(view);
    if let Some(entries) = current_node_entries(app) {
        if let Err(err) = menu.apply_nodes(app, &entries) {
            tracing::warn!(
                code = %err.code,
                error = %err.message,
                "tray node menu sync failed"
            );
        }
    }
}

/// Keep the service switch, the mode group, and the node groups in step with
/// state changes the tray did not make: window actions, recovery, subscription
/// updates, and external OS edits.
pub fn spawn_state_watchdog(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(SYNC_INTERVAL);
        if app.try_state::<AppState>().is_none() {
            // Tauri drops managed state while the app tears down.
            break;
        }
        sync_menu(&app);
    });
}

/// Tray「start/stop proxy service」: same call the Home power button makes.
/// The start can take seconds (elevated TUN, system-proxy apply), so it runs
/// off the main thread; the menu is re-synced from reality afterwards.
fn toggle_service(app: &AppHandle) {
    let Some(menu) = app.try_state::<TrayMenuState>() else {
        return;
    };
    let engaged = menu.service_on();
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        let result = if engaged {
            disable_active_backend_inner(&app, state.inner())
        } else {
            start_service(&app, state.inner())
        };
        if let Err(err) = result {
            tracing::warn!(
                code = %err.code,
                error = %err.message,
                engaged,
                "tray proxy service switch failed"
            );
        }
        // Re-sync the menu and let the window re-read status/settings. Always
        // announced: a failed transition can still leave a different state.
        broadcast_state_change(&app);
    });
}

/// Tray「nodes」: switch the active exit, the same call the Nodes page makes —
/// flat profiles pick a node, grouped profiles switch one group member. Both
/// persist the pick (so it survives a restart) and apply it live when the core
/// runs, so this goes off the main thread like the other tray actions.
fn switch_node(app: &AppHandle, action: NodeAction) {
    if let Some(menu) = app.try_state::<TrayMenuState>() {
        // The platform toggles the clicked check item before the event reaches
        // us; have the next sync rebuild the group from reality.
        menu.nodes_dirty.store(true, Ordering::SeqCst);
    }
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        let result = lock_orchestrate(state.inner()).and_then(|_orch| match &action {
            NodeAction::SelectNode(tag) => select_node(&app, state.inner(), tag),
            NodeAction::SelectMember { group, member } => {
                select_group_member(&app, state.inner(), group, member)
            }
        });
        if let Err(err) = result {
            tracing::warn!(
                code = %err.code,
                error = %err.message,
                ?action,
                "tray node switch failed"
            );
        }
        // Re-sync the menu and let the window re-read status/settings. Always
        // announced: a failed switch can still leave a different state, and the
        // group the platform unchecked on click must be re-derived from reality.
        broadcast_state_change(&app);
    });
}

/// Tray「proxy mode」: same call the Home mode selector makes.
fn switch_mode(app: &AppHandle, mode: ProxyMode) {
    if let Some(menu) = app.try_state::<TrayMenuState>() {
        // The platform toggles the clicked radio-style item before the event
        // reaches us; re-assert the group so a re-click of the active mode
        // cannot leave it unchecked.
        menu.apply_mode(mode);
    }
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        if let Err(err) = apply_proxy_mode(&app, state.inner(), mode) {
            tracing::warn!(
                code = %err.code,
                error = %err.message,
                mode = ?mode,
                "tray proxy mode switch failed"
            );
        }
        broadcast_state_change(&app);
    });
}

pub fn set_language(app: &AppHandle, language: TrayLanguage) -> Result<(), AppError> {
    let state = app
        .try_state::<TrayMenuState>()
        .ok_or_else(|| AppError::new(ErrorCode::ConfigInvalid, "tray menu state is unavailable"))?;
    state.apply_language(language)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_labels_cover_supported_languages() {
        let zh = labels(TrayLanguage::Zh);
        assert_eq!(zh.service(false), "启动代理服务");
        assert_eq!(zh.service(true), "停止代理服务");
        assert_eq!(zh.mode, "代理模式");
        assert_eq!(
            (zh.mode_rule, zh.mode_global, zh.mode_direct),
            ("规则", "全局", "直连")
        );
        assert_eq!(zh.nodes, "节点");
        assert_eq!((zh.show, zh.quit), ("显示", "退出"));

        let en = labels(TrayLanguage::En);
        assert_eq!(en.service(false), "Start Proxy Service");
        assert_eq!(en.service(true), "Stop Proxy Service");
        assert_eq!(en.mode, "Proxy Mode");
        assert_eq!(
            (en.mode_rule, en.mode_global, en.mode_direct),
            ("Rule", "Global", "Direct")
        );
        assert_eq!(en.nodes, "Nodes");
        assert_eq!((en.show, en.quit), ("Show", "Quit"));
    }

    #[test]
    fn menu_codes_round_trip() {
        for mode in [ProxyMode::Rule, ProxyMode::Global, ProxyMode::Direct] {
            assert_eq!(mode_from_code(mode_code(mode)), mode);
        }
        for language in [TrayLanguage::Zh, TrayLanguage::En] {
            assert_eq!(TrayLanguage::from_code(language.code()), language);
        }
    }

    fn node(tag: &str, outbound_type: &str, now: Option<&str>, all: Option<&[&str]>) -> NodeInfo {
        NodeInfo {
            tag: tag.to_string(),
            outbound_type: outbound_type.to_string(),
            group_now: now.map(str::to_string),
            group_all: all.map(|members| members.iter().map(|m| m.to_string()).collect()),
        }
    }

    fn item(entry: &NodeMenuEntry) -> &NodeMenuItem {
        match entry {
            NodeMenuEntry::Item(item) => item,
            other => panic!("expected a node item, got {other:?}"),
        }
    }

    fn group(entry: &NodeMenuEntry) -> &NodeMenuGroup {
        match entry {
            NodeMenuEntry::Group(group) => group,
            other => panic!("expected a node group, got {other:?}"),
        }
    }

    #[test]
    fn flat_profiles_list_every_node_with_the_selected_one_checked() {
        let nodes = vec![
            node("香港 01", "socks", None, None),
            node("日本 02", "vmess", None, None),
        ];
        let entries = node_menu_entries(&nodes, "日本 02");
        assert_eq!(entries.len(), 2);
        assert_eq!(item(&entries[0]).label, "香港 01");
        assert!(!item(&entries[0]).checked);
        assert!(item(&entries[0]).enabled);
        assert_eq!(
            item(&entries[0]).action,
            NodeAction::SelectNode("香港 01".into())
        );
        assert!(item(&entries[1]).checked);
        assert_eq!(
            item(&entries[1]).action,
            NodeAction::SelectNode("日本 02".into())
        );
    }

    #[test]
    fn grouped_profiles_nest_members_and_mirror_the_live_pick() {
        let nodes = vec![
            node(
                "节点选择",
                "selector",
                Some("日本 02"),
                Some(&["香港 01", "日本 02"]),
            ),
            node(
                "自动选择",
                "urltest",
                Some("香港 01"),
                Some(&["香港 01", "日本 02"]),
            ),
            node("空组", "selector", None, Some(&[])),
            node("香港 01", "socks", None, None),
            node("日本 02", "vmess", None, None),
        ];
        let entries = node_menu_entries(&nodes, "香港 01");
        // Empty groups are dropped, and grouped profiles never list bare nodes.
        assert_eq!(entries.len(), 2);

        let selector = group(&entries[0]);
        assert_eq!(selector.label, "节点选择 → 日本 02");
        assert_eq!(selector.members.len(), 2);
        assert!(!selector.members[0].checked);
        assert!(selector.members[0].enabled);
        assert_eq!(
            selector.members[0].action,
            NodeAction::SelectMember {
                group: "节点选择".into(),
                member: "香港 01".into(),
            }
        );
        assert!(selector.members[1].checked);

        // A urltest group picks its member itself: shown, checked, not clickable.
        let urltest = group(&entries[1]);
        assert_eq!(urltest.label, "自动选择 → 香港 01");
        assert!(urltest.members[0].checked);
        assert!(!urltest.members[0].enabled);
    }

    #[test]
    fn node_menu_ids_round_trip_for_awkward_tags() {
        for action in [
            NodeAction::SelectNode("香港:01|A&B [1]".into()),
            NodeAction::SelectMember {
                group: "组:1".into(),
                member: "节点 | 2".into(),
            },
        ] {
            assert_eq!(node_action_from_menu_id(&action.menu_id()), Some(action));
        }
        // Everything else keeps its own id space.
        for id in ["service", "show", "quit", "[\"group\",0]", "[\"member\"]"] {
            assert_eq!(node_action_from_menu_id(id), None, "{id}");
        }
    }

    #[test]
    fn windows_and_macos_menu_text_escapes_ampersands() {
        if cfg!(any(target_os = "windows", target_os = "macos")) {
            assert_eq!(menu_text("A&B"), "A&&B");
        } else {
            assert_eq!(menu_text("A&B"), "A&B");
        }
        assert_eq!(menu_text("香港 01"), "香港 01");
    }

    #[test]
    fn explicit_preferences_select_initial_tray_language() {
        assert_eq!(TrayLanguage::from(LanguagePreference::Zh), TrayLanguage::Zh);
        assert_eq!(TrayLanguage::from(LanguagePreference::En), TrayLanguage::En);
        assert_eq!(
            TrayLanguage::from(LanguagePreference::System),
            TrayLanguage::En
        );
    }
}
