// SPDX-License-Identifier: GPL-3.0-or-later

//! System tray: close → hide; left click → show; Quit → Stop then exit.
//! On macOS the icon also carries the live traffic speed to its right.
//!
//! The menu also carries the actions that do not need the window: the proxy
//! service switch (labeled with the action, like the Home power button), the
//! routing mode group, the subscription group, and the node groups. A watchdog
//! re-derives all of them from the runtime state, so the menu follows changes
//! made anywhere else — window, recovery, or a manual OS edit.

use crate::capture::TrafficCapture;
use crate::commands::{
    apply_after_subscription_change, apply_proxy_mode, broadcast_state_change, collect_nodes,
    current_settings, disable_active_backend_inner, lock_orchestrate, proxy_service_posture,
    select_group_member, select_node, start_service, NodeInfo,
};
use crate::core_snapshot::APP_STATE_CHANGED;
use crate::shutdown::{request_tray_quit, QuitOutcome};
use crate::AppState;
use ice_config::{AppError, ErrorCode, LanguagePreference, ProxyMode};
use ice_core::CoreStatus;
use ice_engine::{read_index, set_active, SubscriptionMeta, SubscriptionPaths};
use serde::Deserialize;
use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{
    menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, Wry,
};
use uuid::Uuid;

/// Menu re-derivation cadence. Longer than the 2s status poll on purpose: the
/// live OS-proxy probe spawns `networksetup` subprocesses on macOS, and the
/// menu tolerates a few seconds of lag (it is re-derived right after every tray
/// action anyway). The `proxy_applied_cache` is shared with the poll, so while
/// the window is open the probe is usually a cache hit.
const SYNC_INTERVAL: Duration = Duration::from_secs(5);

/// Tray icon id. The macOS speed readout looks the icon up by id instead of
/// holding a handle, so the managed menu state stays free of the platform icon.
pub(crate) const TRAY_ID: &str = "ice-box";

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
            LanguagePreference::En => Self::En,
            // Same rule as the web UI (`navigator.language.startsWith("zh")`):
            // resolve against the OS now, so a Chinese system does not flash
            // English labels until the window calls `set_tray_language`.
            LanguagePreference::System => {
                if system_locale_is_chinese() {
                    Self::Zh
                } else {
                    Self::En
                }
            }
        }
    }
}

/// Whether a locale / language tag is Chinese. Mirrors the web UI: the primary
/// subtag is `zh` (or the Windows `Chinese_*` form).
fn locale_tag_is_chinese(tag: &str) -> bool {
    let tag = tag.trim();
    if tag.is_empty() {
        return false;
    }
    let primary = tag.split(['-', '_', '.', '@']).next().unwrap_or(tag);
    primary.eq_ignore_ascii_case("zh") || primary.eq_ignore_ascii_case("chinese")
}

fn system_locale_is_chinese() -> bool {
    let from_tag = locale_tag_is_chinese(&system_locale_tag());
    #[cfg(windows)]
    {
        // Display language can be Chinese while the locale name is `en-US`
        // (formats only). Either source matching `zh` is enough.
        from_tag || windows_ui_language_is_chinese()
    }
    #[cfg(not(windows))]
    {
        from_tag
    }
}

/// Windows display language. Complements [`system_locale_tag`]: WebView2's
/// `navigator.language` usually follows this, not the format locale.
#[cfg(windows)]
fn windows_ui_language_is_chinese() -> bool {
    const LANG_CHINESE: u16 = 0x04;
    const PRIMARYLANGID_MASK: u16 = 0x3ff;
    // SAFETY: `GetUserDefaultUILanguage` is a pure kernel32 read of the
    // current user's UI language; it has no pointers to keep alive.
    let langid = unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() };
    langid & PRIMARYLANGID_MASK == LANG_CHINESE
}

/// BCP-47 locale name (`zh-CN`), the same shape [`locale_tag_is_chinese`]
/// already parses for macOS / Unix tags.
#[cfg(windows)]
fn system_locale_tag() -> String {
    const LOCALE_NAME_MAX_LENGTH: usize = 85;
    let mut buf = [0u16; LOCALE_NAME_MAX_LENGTH];
    // SAFETY: the buffer is `LOCALE_NAME_MAX_LENGTH` wide chars, which is
    // what the API documents; a positive return includes the trailing NUL.
    let n = unsafe {
        windows_sys::Win32::Globalization::GetUserDefaultLocaleName(
            buf.as_mut_ptr(),
            buf.len() as i32,
        )
    };
    if n <= 1 {
        return "en".into();
    }
    String::from_utf16_lossy(&buf[..(n as usize - 1)])
}

#[cfg(target_os = "macos")]
fn system_locale_tag() -> String {
    use objc2_foundation::NSLocale;
    // `preferredLanguages` returns an immutable copy of the user's language
    // list; the first entry is what WKWebView reports as `navigator.language`.
    NSLocale::preferredLanguages()
        .firstObject()
        .map(|s| s.to_string())
        .unwrap_or_else(|| "en".into())
}

#[cfg(not(any(windows, target_os = "macos")))]
fn system_locale_tag() -> String {
    std::env::var("LC_ALL")
        .ok()
        .filter(|value| !value.is_empty() && value != "C" && value != "POSIX")
        .or_else(|| std::env::var("LC_MESSAGES").ok())
        .or_else(|| std::env::var("LANG").ok())
        .unwrap_or_else(|| "en".into())
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
    subs: &'static str,
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
            subs: "订阅",
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
            subs: "Subscriptions",
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

/// One check item of the subscription submenu.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SubscriptionMenuItem {
    id: Uuid,
    label: String,
    checked: bool,
    /// The active entry is not clickable: the tray switches subscriptions, and
    /// re-activating the live one would re-apply the running config for nothing
    /// (turning the active subscription off stays on the Subscriptions page).
    enabled: bool,
}

impl SubscriptionMenuItem {
    /// Menu id carrying the subscription id. Encoded as a JSON array like the
    /// node actions, so the click path needs no shared map to look the entry up
    /// (it must never block the main thread) and the id space stays disjoint.
    fn menu_id(&self) -> String {
        serde_json::json!(["sub", self.id.to_string()]).to_string()
    }
}

/// Decode an id built by [`SubscriptionMenuItem::menu_id`]. `None` for every
/// other menu id (service switch, mode group, node items, submenu parents, …).
fn subscription_id_from_menu_id(id: &str) -> Option<Uuid> {
    if !id.starts_with('[') {
        return None;
    }
    let parts: Vec<String> = serde_json::from_str(id).ok()?;
    match parts.as_slice() {
        [kind, id] if kind == "sub" => Uuid::parse_str(id).ok(),
        _ => None,
    }
}

/// Derive the subscription submenu body: one check item per stored
/// subscription, the active one checked. The Subscriptions page lists the same
/// entries; a pick here mirrors its switch by making that subscription the only
/// active one and applying the change.
fn subscription_menu_entries(metas: &[SubscriptionMeta]) -> Vec<SubscriptionMenuItem> {
    metas
        .iter()
        .map(|meta| SubscriptionMenuItem {
            id: meta.id,
            label: meta.name.clone(),
            checked: meta.active,
            enabled: !meta.active,
        })
        .collect()
}

/// muda reads `&` as a mnemonic marker on Windows and strips a lone `&` on
/// macOS; doubling keeps node tags and subscription names containing `&`
/// literal. GTK uses the text as-is.
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
    subs: Submenu<Wry>,
    /// Body currently attached to `subs`, with the same locking rule as
    /// `nodes_model`.
    subs_model: Mutex<Vec<SubscriptionMenuItem>>,
    /// Set by a subscription click, same reason as `nodes_dirty`: the platform
    /// toggles the clicked check item itself, so a failed or repeated pick must
    /// not leave a wrong check mark behind. Written from the menu thread (no
    /// lock), consumed by the sync.
    subs_dirty: AtomicBool,
    show: MenuItem<Wry>,
    quit: MenuItem<Wry>,
    language: AtomicU8,
    service_on: AtomicBool,
    /// Set for the life of one start/stop so a second click cannot queue the
    /// same action (the label is read before the worker runs).
    service_busy: AtomicBool,
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

    fn service_busy(&self) -> bool {
        self.service_busy.load(Ordering::SeqCst)
    }

    /// `true` if this click owns the switch. A second click while the first
    /// worker is still running is ignored, like the Home power button.
    fn try_begin_service_switch(&self) -> bool {
        !self.service_busy.swap(true, Ordering::SeqCst)
    }

    fn end_service_switch(&self) {
        self.service_busy.store(false, Ordering::SeqCst);
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
        self.update_label("subscriptions", self.subs.set_text(labels.subs))?;
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
        // Win32 `TrackPopupMenu` runs a nested message loop; muda hops menu
        // mutations onto that thread, so tearing the HMENU down while the
        // popup is open can dismiss it or corrupt the menu. A click sets
        // `nodes_dirty` and needs a rebuild even then (the platform has
        // already toggled the check mark); live urltest/fallback `now`
        // changes wait until the popup closes.
        if skip_live_node_rebuild(dirty, tray_popup_menu_open()) {
            return Ok(());
        }
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

    /// Attach the derived subscription body, rebuilding the submenu only when
    /// the model changed. Mirrors [`Self::apply_nodes`], including the lock rule:
    /// only ever called off the main thread.
    fn apply_subscriptions(
        &self,
        app: &AppHandle,
        entries: &[SubscriptionMenuItem],
    ) -> Result<(), AppError> {
        let dirty = self.subs_dirty.load(Ordering::SeqCst);
        let mut applied = self
            .subs_model
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !dirty && applied.as_slice() == entries {
            return Ok(());
        }
        clear_subscription_children(&self.subs)?;
        if let Err(err) = append_subscription_items(app, &self.subs, entries) {
            // Leave the model as it was, so the next sync retries the rebuild.
            let _ = clear_subscription_children(&self.subs);
            return Err(err);
        }
        self.subs
            .set_enabled(!entries.is_empty())
            .map_err(|err| tray_error("update tray subscriptions enabled", err))?;
        *applied = entries.to_vec();
        self.subs_dirty.store(false, Ordering::SeqCst);
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

    /// Apply a derived view. The service item's enabled state is always
    /// rewritten so an in-flight switch can disable it and the next sync
    /// can turn it back on.
    fn apply_view(&self, view: TrayView) {
        // Always re-apply enabled: a switch in flight forces the item off
        // even when the platform capability did not change, and the next
        // sync must turn it back on.
        let _ = self.service.set_enabled(service_item_enabled(
            view.service_enabled,
            self.service_busy(),
        ));
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

/// The switch is clickable only when this platform can start capture and no
/// start/stop is already running.
fn service_item_enabled(capability: bool, busy: bool) -> bool {
    capability && !busy
}

/// Skip a live (non-click) node rebuild while the tray popup is open. Click
/// rebuilds still go through: the platform has already toggled the item.
fn skip_live_node_rebuild(dirty: bool, popup_open: bool) -> bool {
    !dirty && popup_open
}

/// Whether the tray thread is showing a popup. Windows only: classic Win32
/// menus are the ones that break if their HMENU is rebuilt mid-loop. macOS
/// and GTK menus update live and are left alone.
fn tray_popup_menu_open() -> bool {
    #[cfg(target_os = "windows")]
    {
        crate::tray_wheel::popup_menu_open()
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
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

/// Drop every child of the subscription submenu.
fn clear_subscription_children(parent: &Submenu<Wry>) -> Result<(), AppError> {
    while parent
        .remove_at(0)
        .map_err(|err| tray_error("clear tray subscriptions", err))?
        .is_some()
    {}
    Ok(())
}

/// Build and attach the subscription submenu body: one check item per
/// subscription, nothing else.
fn append_subscription_items(
    app: &AppHandle,
    parent: &Submenu<Wry>,
    entries: &[SubscriptionMenuItem],
) -> Result<(), AppError> {
    for entry in entries {
        let item = CheckMenuItem::with_id(
            app,
            entry.menu_id(),
            menu_text(&entry.label),
            entry.enabled,
            entry.checked,
            None::<&str>,
        )
        .map_err(|err| tray_error("create tray subscription item", err))?;
        parent
            .append(&item)
            .map_err(|err| tray_error("append tray subscription item", err))?;
    }
    Ok(())
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
    // Windows draws the menu with `TrackPopupMenu`, which ignores the wheel: a
    // menu taller than the screen would leave the node list to the keyboard and
    // the scroll arrows. The hook has to live on the thread that shows the menu
    // — this one.
    #[cfg(target_os = "windows")]
    crate::tray_wheel::install();
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
    // Same seeding rule as the node submenu above: `apply_subscriptions` takes a
    // lock the main thread must never hold while the watchdog rebuilds.
    let sub_entries = current_subscription_entries(app).unwrap_or_default();
    let subs = Submenu::with_id(app, "subs", labels.subs, false)?;
    let subs_model = if sub_entries.is_empty() {
        Vec::new()
    } else {
        match append_subscription_items(app, &subs, &sub_entries) {
            Ok(()) => {
                let _ = subs.set_enabled(true);
                sub_entries
            }
            Err(err) => {
                tracing::warn!(
                    code = %err.code,
                    error = %err.message,
                    "tray subscription menu setup failed"
                );
                let _ = clear_subscription_children(&subs);
                Vec::new()
            }
        }
    };
    let show = MenuItem::with_id(app, "show", labels.show, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", labels.quit, true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[&service, &mode, &nodes, &subs, &separator, &show, &quit],
    )?;
    app.manage(TrayMenuState {
        service,
        mode,
        mode_rule,
        mode_global,
        mode_direct,
        nodes,
        nodes_model: Mutex::new(node_model),
        nodes_dirty: AtomicBool::new(false),
        subs,
        subs_model: Mutex::new(subs_model),
        subs_dirty: AtomicBool::new(false),
        show,
        quit,
        language: AtomicU8::new(language.code()),
        service_on: AtomicBool::new(view.service_on),
        service_busy: AtomicBool::new(false),
        mode_value: AtomicU8::new(mode_code(view.mode)),
    });

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
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
            if let Some(id) = subscription_id_from_menu_id(id) {
                switch_subscription(app, id);
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

/// Derive the subscription submenu body from the stored index. `None` while the
/// app state or the index is unavailable (first launch, teardown): callers keep
/// the previous menu. `read_index` skips the commit lock, like the status poll.
fn current_subscription_entries(app: &AppHandle) -> Option<Vec<SubscriptionMenuItem>> {
    let state = app.try_state::<AppState>()?;
    let paths = SubscriptionPaths::from_app(&state.paths);
    let index = read_index(&paths).ok()?;
    Some(subscription_menu_entries(&index.items))
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
/// subscription index plus the node list (a cached profile plus, while the core
/// runs, one Clash API call).
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
    if let Some(entries) = current_subscription_entries(app) {
        if let Err(err) = menu.apply_subscriptions(app, &entries) {
            tracing::warn!(
                code = %err.code,
                error = %err.message,
                "tray subscription menu sync failed"
            );
        }
    }
    // Skip the Clash `/proxies` fetch as well as the rebuild: both are wasted
    // while the popup is open, and the fetch is what makes urltest `now`
    // churn trigger a teardown.
    let skip_nodes = skip_live_node_rebuild(
        menu.nodes_dirty.load(Ordering::SeqCst),
        tray_popup_menu_open(),
    );
    if !skip_nodes {
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
}

/// Keep the service switch, the mode group, the subscription group, and the
/// node groups in step with state changes the tray did not make: window actions,
/// recovery, subscription updates, and external OS edits.
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
    // Claim the switch before reading the label: two clicks must not both
    // see "off" and queue two starts (or two stops).
    if !menu.try_begin_service_switch() {
        return;
    }
    let engaged = menu.service_on();
    let _ = menu.service.set_enabled(false);
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Clears `service_busy` then re-syncs, so `apply_view` can re-enable
        // the item. `start_service` / `disable_active_backend_inner` already
        // announced the window on success, while busy was still set; this
        // drop only rebuilds the menu.
        struct ServiceSwitchGuard {
            app: AppHandle,
        }
        impl Drop for ServiceSwitchGuard {
            fn drop(&mut self) {
                if let Some(menu) = self.app.try_state::<TrayMenuState>() {
                    menu.end_service_switch();
                }
                sync_menu(&self.app);
            }
        }
        let _guard = ServiceSwitchGuard { app: app.clone() };
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
            // Inner start/stop only announce on success. Tell the window so a
            // failed switch can still surface a warning; the drop re-enables
            // the item afterwards.
            let _ = app.emit(APP_STATE_CHANGED, ());
        }
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

/// Tray「subscriptions」: make the clicked subscription the active one — the same
/// pick the Subscriptions page switch makes. The pick persists for the next
/// start and is applied live (the running core reloads onto the new profile,
/// which also replaces the node submenu), so it goes off the main thread like
/// the other tray actions.
fn switch_subscription(app: &AppHandle, id: Uuid) {
    if let Some(menu) = app.try_state::<TrayMenuState>() {
        // The platform toggles the clicked check item before the event reaches
        // us; have the next sync rebuild the group from reality.
        menu.subs_dirty.store(true, Ordering::SeqCst);
    }
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        let result = (|| -> Result<(), AppError> {
            let _orch = lock_orchestrate(state.inner())?;
            let paths = SubscriptionPaths::from_app(&state.paths);
            set_active(&paths, id, true).map_err(AppError::from)?;
            let settings = current_settings(&state.paths)?;
            if let Some(err) = apply_after_subscription_change(&app, state.inner(), &settings) {
                tracing::warn!(
                    code = %err.code,
                    error = %err.message,
                    "tray subscription switch apply warning"
                );
            }
            Ok(())
        })();
        if let Err(err) = result {
            tracing::warn!(
                code = %err.code,
                error = %err.message,
                %id,
                "tray subscription switch failed"
            );
        }
        // Re-sync the menu and let the window re-read status/settings. Always
        // announced: a failed switch can still leave a different state, and the
        // group the platform checked on click must be re-derived from reality.
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
            // `apply_proxy_mode` already announced after a persist. A failure
            // before that still needs the menu rebuilt: `apply_mode` above
            // checked the clicked item and reality may not have moved.
            sync_menu(&app);
        }
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
        assert_eq!(zh.subs, "订阅");
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
        assert_eq!(en.subs, "Subscriptions");
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

    fn subscription(id: &str, name: &str, active: bool) -> SubscriptionMeta {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "name": name,
            "url": "https://example.com/sub",
            "active": active,
            "format": "sing_box",
            "node_count": 2,
        }))
        .expect("subscription meta")
    }

    #[test]
    fn subscription_menu_marks_the_active_one_and_keeps_it_unclickable() {
        let first = "11111111-1111-4111-8111-111111111111";
        let second = "22222222-2222-4222-8222-222222222222";
        let metas = vec![
            subscription(first, "机场 A", false),
            subscription(second, "机场 B & C", true),
        ];
        let entries = subscription_menu_entries(&metas);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].label, "机场 A");
        assert!(!entries[0].checked);
        assert!(entries[0].enabled);
        assert_eq!(entries[1].label, "机场 B & C");
        assert!(entries[1].checked);
        assert!(!entries[1].enabled);
    }

    #[test]
    fn subscription_menu_ids_round_trip_and_keep_their_own_id_space() {
        let id = Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap();
        let item = SubscriptionMenuItem {
            id,
            label: "机场 A".into(),
            checked: true,
            enabled: false,
        };
        assert_eq!(item.menu_id(), format!("[\"sub\",\"{id}\"]"));
        assert_eq!(subscription_id_from_menu_id(&item.menu_id()), Some(id));
        // Node items and plain ids stay in their own space.
        assert_eq!(subscription_id_from_menu_id("service"), None);
        assert_eq!(subscription_id_from_menu_id("[\"node\",\"香港 01\"]"), None);
        assert_eq!(
            subscription_id_from_menu_id("[\"sub\",\"not-a-uuid\"]"),
            None
        );
        assert_eq!(subscription_id_from_menu_id("[\"sub\"]"), None);
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
            if system_locale_is_chinese() {
                TrayLanguage::Zh
            } else {
                TrayLanguage::En
            }
        );
    }

    #[test]
    fn locale_tags_treat_chinese_like_the_web_ui() {
        assert!(locale_tag_is_chinese("zh"));
        assert!(locale_tag_is_chinese("zh-CN"));
        assert!(locale_tag_is_chinese("zh_TW"));
        assert!(locale_tag_is_chinese("zh-Hans-CN"));
        assert!(locale_tag_is_chinese("zh_CN.UTF-8"));
        assert!(locale_tag_is_chinese("Chinese_China.936"));
        assert!(!locale_tag_is_chinese("en"));
        assert!(!locale_tag_is_chinese("en-US"));
        assert!(!locale_tag_is_chinese("ja-JP"));
        assert!(!locale_tag_is_chinese("C"));
        assert!(!locale_tag_is_chinese(""));
    }

    #[test]
    fn service_item_is_disabled_while_a_switch_is_in_flight() {
        assert!(service_item_enabled(true, false));
        assert!(!service_item_enabled(true, true));
        assert!(!service_item_enabled(false, false));
        assert!(!service_item_enabled(false, true));
    }

    #[test]
    fn live_node_rebuild_waits_while_the_popup_is_open() {
        // A watchdog tick with no click must not tear the HMENU down.
        assert!(skip_live_node_rebuild(false, true));
        // A click still rebuilds: the platform already toggled the check mark.
        assert!(!skip_live_node_rebuild(true, true));
        assert!(!skip_live_node_rebuild(false, false));
        assert!(!skip_live_node_rebuild(true, false));
    }
}
