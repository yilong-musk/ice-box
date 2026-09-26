// SPDX-License-Identifier: GPL-3.0-or-later

//! System tray: close → hide; left click → show; Quit → Stop then exit.
//! On macOS the icon also carries the live traffic speed to its right.
//!
//! The menu also carries the actions that do not need the window: the proxy
//! service switch (labeled with the action, like the Home power button), the
//! routing mode group, the subscription group, the node groups, and the
//! session-only Mixed terminal helpers (copy command / open terminal). A watchdog
//! re-derives all of them from the runtime state, so the menu follows changes
//! made anywhere else — window, recovery, or a manual OS edit.
//!
//! While an update is waiting, the window mirrors the sidebar arrow into the
//! menu: a green-arrow prompt that opens Settings → App Updates. Native menu
//! text cannot be coloured, so the arrow travels as the item's icon.

use crate::capture::TrafficCapture;
use crate::commands::{
    apply_after_subscription_change, apply_proxy_mode, broadcast_state_change, collect_nodes,
    copy_proxy_terminal_command, current_settings, disable_active_backend_inner, lock_orchestrate,
    open_proxy_terminal_from_state, proxy_service_posture, select_group_member, select_node,
    start_service, NodeInfo,
};
use crate::core_snapshot::APP_STATE_CHANGED;
use crate::shutdown::{request_tray_quit, QuitOutcome};
#[cfg(any(target_os = "macos", test))]
use crate::tray_delay::{delay_tone, DelayOutcome, DelayScope, DelayTone, DelayView};
use crate::AppState;
use ice_config::{AppError, ErrorCode, LanguagePreference, ProxyMode};
use ice_core::CoreStatus;
use ice_engine::{read_index, set_active, SubscriptionMeta, SubscriptionPaths};
#[cfg(target_os = "macos")]
use objc2::rc::Retained;
#[cfg(target_os = "macos")]
use objc2::AllocAnyThread;
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSColor, NSForegroundColorAttributeName, NSMenu, NSMenuItem};
#[cfg(target_os = "macos")]
use objc2_foundation::{NSMutableAttributedString, NSRange, NSString};
use serde::Deserialize;
use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{
    image::Image,
    menu::{
        CheckMenuItem, IconMenuItem, IsMenuItem, Menu, MenuItem, MenuItemKind, PredefinedMenuItem,
        Submenu,
    },
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

/// Emitted when the tray update prompt is clicked: the window opens Settings →
/// App Updates, the same destination as the sidebar arrow.
pub const UPDATE_PROMPT_EVENT: &str = "app-update://open";

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
    copy_cli_proxy: &'static str,
    open_cli_proxy: &'static str,
    show: &'static str,
    quit: &'static str,
    /// Delay test button of a node page: the top page (each group's current
    /// exit, or every node on a flat profile) and a group page (the group's
    /// members) carry the same label.
    delay_test: &'static str,
    /// Template of the button of the page whose test is in flight; the two
    /// `{}` are the probe in flight and the total.
    delay_progress: &'static str,
    /// Suffix of a row whose probe failed.
    delay_failed: &'static str,
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

    /// Text of the button whose page has a test in flight: `延迟测试中 3/8`.
    #[cfg(any(target_os = "macos", test))]
    fn delay_progress_text(self, index: usize, total: usize) -> String {
        self.delay_progress
            .replacen("{}", &index.to_string(), 1)
            .replacen("{}", &total.to_string(), 1)
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
            copy_cli_proxy: "复制代理命令",
            open_cli_proxy: "打开代理终端",
            show: "显示",
            quit: "退出",
            delay_test: "延迟测试",
            delay_progress: "延迟测试中 {}/{}",
            delay_failed: "失败",
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
            copy_cli_proxy: "Copy Proxy Command",
            open_cli_proxy: "Open Proxy Terminal",
            show: "Show",
            quit: "Quit",
            delay_test: "Test Delay",
            delay_progress: "Testing {}/{}",
            delay_failed: "Failed",
        },
    }
}

/// Text of the update prompt. Short on purpose: the menu is narrow, and the
/// click already lands on Settings → App Updates.
fn update_prompt_text(language: TrayLanguage, version: &str) -> String {
    match language {
        TrayLanguage::Zh => format!("有新版本 {version}"),
        TrayLanguage::En => format!("Version {version} is available"),
    }
}

/// Menu icon size. Win32 draws menu bitmaps at 16×16, and muda sizes the macOS
/// `NSImage` from the pixels, so one bitmap serves both.
const UPDATE_ARROW_SIZE: u32 = 16;

/// The sidebar arrow's colour (`text-green-500` in the web UI).
const UPDATE_ARROW_GREEN: [u8; 3] = [34, 197, 94];

/// Green up-arrow on a transparent bitmap: the tray twin of the sidebar's
/// arrow. Native menu text cannot be coloured, so this is what carries the
/// green on both platforms.
fn update_arrow_icon() -> Image<'static> {
    const SAMPLES: u32 = 4;
    let size = UPDATE_ARROW_SIZE;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let mut covered = 0;
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let u = (x as f32 + (sx as f32 + 0.5) / SAMPLES as f32) / size as f32;
                    let v = (y as f32 + (sy as f32 + 0.5) / SAMPLES as f32) / size as f32;
                    if arrow_covers(u, v) {
                        covered += 1;
                    }
                }
            }
            if covered == 0 {
                continue;
            }
            // 4×4 supersampling: the arrow keeps a smooth edge at menu size
            // without pulling in an image crate.
            let alpha = (covered * 255 / (SAMPLES * SAMPLES)) as u8;
            let offset = ((y * size + x) * 4) as usize;
            rgba[offset..offset + 3].copy_from_slice(&UPDATE_ARROW_GREEN);
            rgba[offset + 3] = alpha;
        }
    }
    Image::new_owned(rgba, size, size)
}

/// Point test for the arrow inside a unit box: a head triangle from the apex
/// down to the base, over a shaft that runs to the bottom edge.
fn arrow_covers(u: f32, v: f32) -> bool {
    const APEX_V: f32 = 0.08;
    const BASE_V: f32 = 0.52;
    const HEAD_HALF_WIDTH: f32 = 0.38;
    const SHAFT_HALF_WIDTH: f32 = 0.14;
    const SHAFT_TOP: f32 = 0.44;
    const BOTTOM: f32 = 0.94;
    if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
        return false;
    }
    if (SHAFT_TOP..=BOTTOM).contains(&v) && (u - 0.5).abs() <= SHAFT_HALF_WIDTH {
        return true;
    }
    if (APEX_V..=BASE_V).contains(&v) {
        let half = HEAD_HALF_WIDTH * (v - APEX_V) / (BASE_V - APEX_V);
        return (u - 0.5).abs() <= half;
    }
    false
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

    /// The tag the row stands for: the node it picks, or the member it switches
    /// its group to. What the delay decoration looks the newest result up by.
    #[cfg(any(target_os = "macos", test))]
    fn tag(&self) -> &str {
        match self {
            Self::SelectNode(tag) => tag,
            Self::SelectMember { member, .. } => member,
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
    /// Tag of the group itself; the group page's test button names it.
    #[cfg(any(target_os = "macos", test))]
    tag: String,
    label: String,
    /// Live member the group exits through, when it has one. The group label
    /// carries that member's delay result.
    #[cfg(any(target_os = "macos", test))]
    now: Option<String>,
    /// Delay test button at the top of the group page (macOS-only feature).
    #[cfg(any(target_os = "macos", test))]
    delay: Option<DelayButton>,
    members: Vec<NodeMenuItem>,
}

impl NodeMenuGroup {
    fn new(tag: &str, now: Option<&str>, members: Vec<NodeMenuItem>) -> Self {
        Self {
            label: group_label(tag, now),
            #[cfg(any(target_os = "macos", test))]
            tag: tag.to_string(),
            #[cfg(any(target_os = "macos", test))]
            now: now.map(str::to_string),
            #[cfg(any(target_os = "macos", test))]
            delay: None,
            members,
        }
    }
}

/// Node submenu body. Derived from the active profile and the live selections;
/// the submenu is rebuilt only when this value changes.
#[derive(Debug, Clone, PartialEq, Eq)]
enum NodeMenuEntry {
    /// Page-level delay test button (macOS-only feature).
    #[cfg(any(target_os = "macos", test))]
    Delay(DelayButton),
    Item(NodeMenuItem),
    Group(NodeMenuGroup),
}

/// One page's delay test button, as the menu should draw it now.
#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Clone, PartialEq, Eq)]
struct DelayButton {
    label: String,
    enabled: bool,
    scope: DelayScope,
}

/// Rows one page's delay button brings with it: the item itself, and the
/// separator under it.
#[cfg(any(target_os = "macos", test))]
const DELAY_BUTTON_ROWS: usize = 2;

/// A rendered page button: the item, and the separator under it.
#[cfg(any(target_os = "macos", test))]
struct DelayRows {
    button: MenuItem<Wry>,
    separator: PredefinedMenuItem<Wry>,
}

#[cfg(any(target_os = "macos", test))]
impl DelayRows {
    fn refs(&self) -> [&dyn IsMenuItem<Wry>; 2] {
        [&self.button, &self.separator]
    }
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
            Some(NodeMenuEntry::Group(NodeMenuGroup::new(
                &group.tag,
                now,
                members
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
            )))
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
    cli_proxy_enabled: bool,
}

impl Default for TrayView {
    fn default() -> Self {
        Self {
            service_on: false,
            // A failed read must not disable the switch; the next sync decides.
            service_enabled: true,
            mode: ProxyMode::Rule,
            cli_proxy_enabled: true,
        }
    }
}

/// The update prompt the window reports and the label the menu carries. The
/// pair is compared as rendered text, so a language switch re-texts the item
/// like any other change.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct UpdatePrompt {
    /// Version the window last reported as available, if any.
    wanted: Option<String>,
    /// Label attached to the menu item, if it is in the menu.
    shown: Option<String>,
}

/// What the menu must do to move the prompt from `shown` to `wanted`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpdatePromptStep {
    /// The menu already shows what the window asked for.
    Settled,
    /// Attach the item.
    Attach,
    /// Rewrite the label of the attached item.
    Retext,
    /// Detach the item.
    Detach,
}

fn update_prompt_step(wanted: Option<&str>, shown: Option<&str>) -> UpdatePromptStep {
    match (wanted, shown) {
        (None, None) => UpdatePromptStep::Settled,
        (None, Some(_)) => UpdatePromptStep::Detach,
        (Some(wanted), Some(shown)) if wanted == shown => UpdatePromptStep::Settled,
        (Some(_), Some(_)) => UpdatePromptStep::Retext,
        (Some(_), None) => UpdatePromptStep::Attach,
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
    copy_cli_proxy: MenuItem<Wry>,
    open_cli_proxy: MenuItem<Wry>,
    show: MenuItem<Wry>,
    quit: MenuItem<Wry>,
    /// The menu itself, so the update prompt can be attached and dropped
    /// without rebuilding the icon.
    menu: Menu<Wry>,
    /// Update prompt item. Kept out of the menu until a version is reported.
    update_item: IconMenuItem<Wry>,
    /// Locked only off the main thread, like `nodes_model`: attaching the item
    /// blocks on main-thread menu mutations, so a main-thread caller holding
    /// this would deadlock against the watchdog.
    update: Mutex<UpdatePrompt>,
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
        self.update_label(
            "copy command",
            self.copy_cli_proxy.set_text(labels.copy_cli_proxy),
        )?;
        self.update_label(
            "open terminal",
            self.open_cli_proxy.set_text(labels.open_cli_proxy),
        )?;
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
        // Rows that are already on the submenu are retitled in place. A
        // rebuild drops every row and re-creates it, and an open tray menu
        // does not survive that: the delay test streams its progress into the
        // menu the click came from, and a closed menu is both a cancel and an
        // end to what the test is showing. A run only retitles the rows it
        // touches, so those updates come through here; a model that lists
        // different rows (another profile, another node) is rebuilt as before.
        if same_node_rows(&applied, entries) && retitle_node_rows(&self.nodes, entries)? {
            *applied = entries.to_vec();
            self.nodes_dirty.store(false, Ordering::SeqCst);
            #[cfg(target_os = "macos")]
            {
                let labels = self.labels();
                colorize_delay_results(app, &labels);
                attach_delay_buttons(app, entries, &labels);
            }
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
        // The rebuild dropped the coloured result suffixes and the delay
        // buttons' custom views with the old rows; put both back on the new
        // ones (`colorize_delay_results`, `attach_delay_buttons`).
        #[cfg(target_os = "macos")]
        {
            let labels = self.labels();
            colorize_delay_results(app, &labels);
            attach_delay_buttons(app, entries, &labels);
        }
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

    /// Record the version the window reports as available (`None` = nothing to
    /// offer) and reconcile the menu item.
    fn apply_update(&self, wanted: Option<String>) -> Result<(), AppError> {
        let mut prompt = self
            .update
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        prompt.wanted = wanted;
        self.reconcile_update(&mut prompt)
    }

    /// Retry a prompt change the Windows popup blocked, or one the window made
    /// before the watchdog started. Only ever called off the main thread.
    fn sync_update(&self) -> Result<(), AppError> {
        let mut prompt = self
            .update
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.reconcile_update(&mut prompt)
    }

    /// Attach, re-text, or drop the prompt item. A skipped change stays
    /// pending (`wanted` and `shown` keep differing) for the next sync.
    fn reconcile_update(&self, prompt: &mut UpdatePrompt) -> Result<(), AppError> {
        let language = TrayLanguage::from_code(self.language.load(Ordering::SeqCst));
        let wanted = prompt
            .wanted
            .as_deref()
            .map(|version| update_prompt_text(language, version));
        let step = update_prompt_step(wanted.as_deref(), prompt.shown.as_deref());
        if step == UpdatePromptStep::Settled {
            return Ok(());
        }
        // Win32 `TrackPopupMenu` runs its own message loop over the open menu;
        // tearing the item down inside it can dismiss the popup. The watchdog
        // retries once it closes, like the node submenu rebuilds.
        if tray_popup_menu_open() {
            return Ok(());
        }
        match step {
            UpdatePromptStep::Attach | UpdatePromptStep::Retext => {
                let Some(label) = wanted else {
                    return Ok(());
                };
                self.update_item
                    .set_text(&label)
                    .map_err(|err| tray_error("update tray update prompt", err))?;
                if step == UpdatePromptStep::Attach {
                    self.menu
                        .insert(&self.update_item, 0)
                        .map_err(|err| tray_error("insert tray update prompt", err))?;
                }
                prompt.shown = Some(label);
            }
            UpdatePromptStep::Detach => {
                self.menu
                    .remove(&self.update_item)
                    .map_err(|err| tray_error("remove tray update prompt", err))?;
                prompt.shown = None;
            }
            UpdatePromptStep::Settled => {}
        }
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
        let _ = self.copy_cli_proxy.set_enabled(view.cli_proxy_enabled);
        let _ = self.open_cli_proxy.set_enabled(view.cli_proxy_enabled);
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
/// submenu per strategy group. A page's delay test button (macOS-only feature)
/// sits above a separator, with the rows under it.
fn append_node_entries(
    app: &AppHandle,
    parent: &Submenu<Wry>,
    entries: &[NodeMenuEntry],
) -> Result<(), AppError> {
    for (index, entry) in entries.iter().enumerate() {
        match entry {
            #[cfg(any(target_os = "macos", test))]
            NodeMenuEntry::Delay(button) => {
                let rows = delay_rows(app, button)?;
                for row in rows.refs() {
                    parent
                        .append(row)
                        .map_err(|err| tray_error("append tray delay button", err))?;
                }
            }
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
                #[cfg(any(target_os = "macos", test))]
                let delay = group
                    .delay
                    .as_ref()
                    .map(|button| delay_rows(app, button))
                    .transpose()?;
                let mut rows: Vec<&dyn IsMenuItem<Wry>> = Vec::new();
                #[cfg(any(target_os = "macos", test))]
                if let Some(delay) = &delay {
                    rows.extend(delay.refs());
                }
                rows.extend(members.iter().map(|member| member as &dyn IsMenuItem<Wry>));
                let submenu = Submenu::with_id_and_items(
                    app,
                    serde_json::json!(["group", index]).to_string(),
                    menu_text(&group.label),
                    true,
                    &rows,
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

/// Whether every entry of `entries` stands for the same row as the entry in
/// its place in `applied`: same kind, same node, same group members. Labels and
/// click states are what a retitle rewrites, so they are not part of a row's
/// identity.
fn same_node_rows(applied: &[NodeMenuEntry], entries: &[NodeMenuEntry]) -> bool {
    applied.len() == entries.len()
        && applied
            .iter()
            .zip(entries)
            .all(|(applied, entry)| same_row_shape(applied, entry))
}

/// One entry of [`same_node_rows`] against the one that stands in its place.
fn same_row_shape(applied: &NodeMenuEntry, entry: &NodeMenuEntry) -> bool {
    match (applied, entry) {
        #[cfg(any(target_os = "macos", test))]
        (NodeMenuEntry::Delay(_), NodeMenuEntry::Delay(_)) => true,
        (NodeMenuEntry::Item(applied), NodeMenuEntry::Item(entry)) => {
            applied.action.menu_id() == entry.action.menu_id()
        }
        (NodeMenuEntry::Group(applied), NodeMenuEntry::Group(entry)) => {
            // A group page is the tag it stands for plus the members it lists;
            // its label carries the live exit and moves with it.
            #[cfg(any(target_os = "macos", test))]
            let same_identity =
                applied.tag == entry.tag && applied.delay.is_some() == entry.delay.is_some();
            #[cfg(not(any(target_os = "macos", test)))]
            let same_identity = true;
            same_identity
                && applied.members.len() == entry.members.len()
                && applied
                    .members
                    .iter()
                    .zip(&entry.members)
                    .all(|(applied, entry)| applied.action.menu_id() == entry.action.menu_id())
        }
        _ => false,
    }
}

/// Rewrite the node submenu's rows from `entries` without touching the rows
/// themselves: the same items, with the labels and states the model now
/// carries. See [`TrayMenuState::apply_nodes`] for why an update the menu is
/// open on must come through here rather than through a rebuild.
///
/// A rewritten row keeps whatever delay colour it carried: `setTitle` (what
/// [`retitle_node_item`] calls) does not touch an attributed title. The caller
/// re-derives the colours after the walk, which also drops the attribute of a
/// row whose result is gone.
///
/// The walk addresses the rows the way [`append_node_entries`] lays them out —
/// the page button and its separator, one check row per item, one submenu per
/// group with the group's own button and members inside — so the two must stay
/// in step. `false` comes back when the submenu does not hold exactly those
/// rows — a rebuild that failed part-way, say — and the caller rebuilds after
/// all, rather than retitling rows that stand for something else.
fn retitle_node_rows(parent: &Submenu<Wry>, entries: &[NodeMenuEntry]) -> Result<bool, AppError> {
    let rows = parent
        .items()
        .map_err(|err| tray_error("read tray node rows", err))?;
    if rows.len() != node_row_count(entries) {
        return Ok(false);
    }
    let mut index = 0usize;
    for entry in entries {
        match entry {
            #[cfg(any(target_os = "macos", test))]
            NodeMenuEntry::Delay(button) => {
                if let Some(MenuItemKind::MenuItem(row)) = rows.get(index) {
                    retitle_delay_row(row, button)?;
                }
                // The separator under the button carries nothing of its own,
                // so the walk steps over it.
                index += DELAY_BUTTON_ROWS;
            }
            NodeMenuEntry::Item(item) => {
                if let Some(MenuItemKind::Check(row)) = rows.get(index) {
                    retitle_node_item(row, item)?;
                }
                index += 1;
            }
            NodeMenuEntry::Group(group) => {
                let Some(MenuItemKind::Submenu(submenu)) = rows.get(index) else {
                    index += 1;
                    continue;
                };
                submenu
                    .set_text(menu_text(&group.label))
                    .map_err(|err| tray_error("retitle tray node group", err))?;
                let members = submenu
                    .items()
                    .map_err(|err| tray_error("read tray node group rows", err))?;
                if members.len() != group_row_count(group) {
                    return Ok(false);
                }
                // The group's own button (and the separator under it) sits
                // above its members on the pages that have one; the walk steps
                // over both to reach them.
                #[cfg(any(target_os = "macos", test))]
                if let Some(button) = &group.delay {
                    if let Some(MenuItemKind::MenuItem(row)) = members.first() {
                        retitle_delay_row(row, button)?;
                    }
                }
                let button_rows = group_delay_rows(group);
                for (offset, member) in group.members.iter().enumerate() {
                    if let Some(MenuItemKind::Check(row)) = members.get(button_rows + offset) {
                        retitle_node_item(row, member)?;
                    }
                }
                index += 1;
            }
        }
    }
    Ok(true)
}

/// How many rows `entries` puts at the top level of the node submenu: one per
/// entry, except a page's delay button, which brings its separator along.
fn node_row_count(entries: &[NodeMenuEntry]) -> usize {
    entries
        .iter()
        .map(|entry| match entry {
            #[cfg(any(target_os = "macos", test))]
            NodeMenuEntry::Delay(_) => DELAY_BUTTON_ROWS,
            NodeMenuEntry::Item(_) | NodeMenuEntry::Group(_) => 1,
        })
        .sum()
}

/// How many rows one group page puts inside its own submenu: its members, and
/// the group's own delay button with its separator where that page has one.
fn group_row_count(group: &NodeMenuGroup) -> usize {
    group.members.len() + group_delay_rows(group)
}

/// How many rows a group page puts above its members: the group's own delay
/// button and its separator, on the pages that have a button.
#[cfg(any(target_os = "macos", test))]
fn group_delay_rows(group: &NodeMenuGroup) -> usize {
    if group.delay.is_some() {
        DELAY_BUTTON_ROWS
    } else {
        0
    }
}

/// Nothing sits above the members where the platform has no delay button.
#[cfg(not(any(target_os = "macos", test)))]
fn group_delay_rows(_group: &NodeMenuGroup) -> usize {
    0
}

/// Rewrite one node row: its text, whether it takes a click, and its check
/// mark (the platform toggles that one itself on a click, so the model's value
/// is re-asserted).
fn retitle_node_item(row: &CheckMenuItem<Wry>, item: &NodeMenuItem) -> Result<(), AppError> {
    row.set_text(menu_text(&item.label))
        .map_err(|err| tray_error("retitle tray node item", err))?;
    row.set_enabled(item.enabled)
        .map_err(|err| tray_error("retitle tray node item enabled", err))?;
    row.set_checked(item.checked)
        .map_err(|err| tray_error("retitle tray node item checked", err))
}

/// Rewrite one page's delay button. The view on the row draws the label, so
/// the text here is what the menu item carries for the platform (and what a
/// screen reader reads); the view itself is re-attached after the walk.
#[cfg(any(target_os = "macos", test))]
fn retitle_delay_row(row: &MenuItem<Wry>, button: &DelayButton) -> Result<(), AppError> {
    row.set_text(menu_text(&button.label))
        .map_err(|err| tray_error("retitle tray delay button", err))?;
    row.set_enabled(button.enabled)
        .map_err(|err| tray_error("retitle tray delay button enabled", err))
}

/// Render a page's delay test button and the separator under it.
#[cfg(any(target_os = "macos", test))]
fn delay_rows(app: &AppHandle, button: &DelayButton) -> Result<DelayRows, AppError> {
    let item = MenuItem::with_id(
        app,
        button.scope.menu_id(),
        menu_text(&button.label),
        button.enabled,
        None::<&str>,
    )
    .map_err(|err| tray_error("create tray delay button", err))?;
    let separator = PredefinedMenuItem::separator(app)
        .map_err(|err| tray_error("create tray delay separator", err))?;
    Ok(DelayRows {
        button: item,
        separator,
    })
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

/// Tray update prompt: bring the window forward and let it open Settings → App
/// Updates — the destination the sidebar arrow uses too, so the user lands on
/// the same card (with the install button) either way.
fn on_open_update(app: &AppHandle) {
    show_main_window(app);
    let _ = app.emit(UPDATE_PROMPT_EVENT, ());
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
    // The delay buttons' views hang off the status item's menu, which exists
    // only once the icon is built at the end of this function: the model is
    // kept aside for that first attach instead of read back under a lock the
    // main thread must not take.
    #[cfg(target_os = "macos")]
    let seeded_node_model = node_model.clone();
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
    let copy_cli_proxy = MenuItem::with_id(
        app,
        "copy-cli-proxy",
        labels.copy_cli_proxy,
        view.cli_proxy_enabled,
        None::<&str>,
    )?;
    let open_cli_proxy = MenuItem::with_id(
        app,
        "open-cli-proxy",
        labels.open_cli_proxy,
        view.cli_proxy_enabled,
        None::<&str>,
    )?;
    let show = MenuItem::with_id(app, "show", labels.show, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", labels.quit, true, None::<&str>)?;
    // Out of the menu until the window reports a version: the label, and the
    // green arrow icon that carries the sidebar's colour, are set on attach.
    let update_item = IconMenuItem::with_id(
        app,
        "update",
        "",
        true,
        Some(update_arrow_icon()),
        None::<&str>,
    )?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[
            &service,
            &mode,
            &nodes,
            &subs,
            &copy_cli_proxy,
            &open_cli_proxy,
            &separator,
            &show,
            &quit,
        ],
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
        copy_cli_proxy,
        open_cli_proxy,
        show,
        quit,
        // A handle to the menu so the prompt can be attached and dropped
        // without rebuilding the icon.
        menu: menu.clone(),
        update_item,
        update: Mutex::new(UpdatePrompt::default()),
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
                "copy-cli-proxy" => on_copy_cli_proxy(app),
                "open-cli-proxy" => on_open_cli_proxy(app),
                "update" => on_open_update(app),
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
    #[cfg(target_os = "macos")]
    {
        crate::tray_delay::install_menu_watch(&_tray);
        attach_delay_buttons(app, &seeded_node_model, &labels);
    }
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
        cli_proxy_enabled: crate::commands::mixed_proxy_endpoint(state.inner()).is_ok(),
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
    let entries = node_menu_entries(&nodes, &selected);
    #[cfg(any(target_os = "macos", test))]
    let entries = decorate_delay(entries, &current_labels(app), &nodes);
    Some(entries)
}

/// Labels for the menu being built: the tray's live language, or the settings'
/// preference while the tray state does not exist yet (menu seeding).
#[cfg(any(target_os = "macos", test))]
fn current_labels(app: &AppHandle) -> TrayLabels {
    if let Some(state) = app.try_state::<TrayMenuState>() {
        return state.labels();
    }
    let language = app
        .try_state::<AppState>()
        .and_then(|state| current_settings(&state.paths).ok())
        .map(|settings| TrayLanguage::from(settings.language))
        .unwrap_or(TrayLanguage::En);
    labels(language)
}

/// Apply the newest delay test to the derived node body: every page gets its
/// test button, and every row the result of the exit node it stands for.
///
/// The button of the page whose test is in flight shows its progress; while any
/// test runs, every button is disabled. Nothing to decorate (and no buttons)
/// while there are no rows — the top page only carries a button when it has
/// nodes.
#[cfg(any(target_os = "macos", test))]
fn decorate_delay(
    mut entries: Vec<NodeMenuEntry>,
    labels: &TrayLabels,
    nodes: &[NodeInfo],
) -> Vec<NodeMenuEntry> {
    apply_delay(
        &mut entries,
        labels,
        nodes,
        &crate::tray_delay::current_view(),
    );
    entries
}

/// The pure half of [`decorate_delay`]: what `view` does to the derived body.
#[cfg(any(target_os = "macos", test))]
fn apply_delay(
    entries: &mut Vec<NodeMenuEntry>,
    labels: &TrayLabels,
    nodes: &[NodeInfo],
    view: &DelayView,
) {
    for entry in entries.iter_mut() {
        match entry {
            NodeMenuEntry::Delay(_) => {}
            NodeMenuEntry::Item(item) => {
                item.label
                    .push_str(&delay_suffix(view, labels, nodes, item.action.tag()));
            }
            NodeMenuEntry::Group(group) => {
                // The label carries the group's live exit, so it carries that
                // exit's result too.
                if let Some(now) = group.now.as_deref() {
                    group
                        .label
                        .push_str(&delay_suffix(view, labels, nodes, now));
                }
                for member in group.members.iter_mut() {
                    member
                        .label
                        .push_str(&delay_suffix(view, labels, nodes, member.action.tag()));
                }
                group.delay = Some(delay_button(
                    view,
                    labels,
                    DelayScope::Group(group.tag.clone()),
                ));
            }
        }
    }
    if !entries.is_empty() {
        let button = delay_button(view, labels, DelayScope::Top);
        entries.insert(0, NodeMenuEntry::Delay(button));
    }
}

/// One page's button: the progress label while its own test runs, the plain
/// label otherwise, and disabled while any test runs.
#[cfg(any(target_os = "macos", test))]
fn delay_button(view: &DelayView, labels: &TrayLabels, scope: DelayScope) -> DelayButton {
    let label = match view.progress(&scope) {
        Some((index, total)) => labels.delay_progress_text(index, total),
        None => labels.delay_test.to_string(),
    };
    DelayButton {
        label,
        enabled: view.idle(),
        scope,
    }
}

/// Separator between a row's name and the delay result appended to it. The
/// colouring side matches this exact text, so the two sides cannot drift.
#[cfg(any(target_os = "macos", test))]
const SUFFIX_SEPARATOR: &str = " · ";

/// Unit of a measured delay, as the suffix prints it.
#[cfg(any(target_os = "macos", test))]
const DELAY_UNIT: &str = " ms";

/// Suffix a row carries for the newest test: nothing when the exit node behind
/// `tag` was not probed, else `…`, the measured delay, or the failure text.
#[cfg(any(target_os = "macos", test))]
fn delay_suffix(view: &DelayView, labels: &TrayLabels, nodes: &[NodeInfo], tag: &str) -> String {
    let Some(exit) = crate::tray_delay::resolve_exit_tag(nodes, tag) else {
        return String::new();
    };
    match view.outcome(&exit) {
        None => String::new(),
        Some(DelayOutcome::Testing) => format!("{SUFFIX_SEPARATOR}…"),
        Some(DelayOutcome::Done(delay_ms)) => format!("{SUFFIX_SEPARATOR}{delay_ms}{DELAY_UNIT}"),
        Some(DelayOutcome::Failed) => format!("{SUFFIX_SEPARATOR}{}", labels.delay_failed),
    }
}

/// The delay result a menu title carries: the colour band it asks for, and the
/// byte offset its suffix starts at. `None` for a title without a finished
/// result — a bare row, the `…` of a probe in flight, or anything else.
///
/// The title is what the menu shows rather than the model behind it: every
/// finished row prints exactly what [`delay_suffix`] appended, and no other
/// item in the tray menu ends in ` · <digits> ms` or ` · <failed>`.
#[cfg(any(target_os = "macos", test))]
fn delay_suffix_color(title: &str, labels: &TrayLabels) -> Option<(usize, DelayTone)> {
    let start = title.rfind(SUFFIX_SEPARATOR)?;
    let suffix = &title[start + SUFFIX_SEPARATOR.len()..];
    // The failure text is a label, not a number: compare the whole suffix.
    if suffix == labels.delay_failed {
        return Some((start, DelayTone::Bad));
    }
    let digits = suffix.strip_suffix(DELAY_UNIT)?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((start, delay_tone(digits.parse().ok()?)))
}

/// `start..` of `title` as the pair AppKit counts in: an `NSRange` holds UTF-16
/// code units, so a node name holding multi-byte characters would misplace the
/// colour if the range were counted in Rust bytes.
#[cfg(any(target_os = "macos", test))]
fn suffix_units(title: &str, start: usize) -> (usize, usize) {
    let location = title[..start].encode_utf16().count();
    let length = title[start..].encode_utf16().count();
    (location, length)
}

/// Colour the delay results of the tray menu after a node rebuild.
///
/// The rebuild is what needs this: muda drops the submenu's rows and builds new
/// ones, and a fresh item carries no attributed title, so every rebuild colours
/// its own rows — the menu that is open during a run is refreshed at most once
/// a second, and each refresh colours the results that have landed.
///
/// Off-main-thread only, like the rebuild itself: the walk hops to the main
/// thread and blocks until it is done, so no rebuild interleaves with it.
#[cfg(target_os = "macos")]
fn colorize_delay_results(app: &AppHandle, labels: &TrayLabels) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let labels = *labels;
    let result = tray.with_inner_tray_icon(move |tray| {
        let Some(mtm) = objc2::MainThreadMarker::new() else {
            return false;
        };
        let Some(status_item) = tray.ns_status_item() else {
            return false;
        };
        let Some(menu) = status_item.menu(mtm) else {
            return false;
        };
        colorize_menu(&menu, labels);
        true
    });
    match result {
        Ok(true) => {}
        Ok(false) => tracing::warn!("tray delay colours: tray menu not found"),
        Err(err) => tracing::warn!(error = %err, "tray delay colours: menu walk failed"),
    }
}

/// Colour the finished-result suffix of every row of `menu`, the submenus (the
/// node groups) included: a group's own title carries the result of the member
/// it exits through, and its members carry their own.
///
/// The walk uncolours as well: retitling a row goes through `setTitle` (muda's
/// `set_text`), which leaves an `attributedTitle` an earlier pass set in place
/// — and AppKit draws that one — so a row whose finished result is gone (the
/// menu closed and dropped it, or a new run replaced it with `…`) would keep
/// showing the old text and colour. Letting the attribute go with the result
/// is what keeps the drawn row and the model behind it the same.
#[cfg(target_os = "macos")]
fn colorize_menu(menu: &NSMenu, labels: TrayLabels) {
    let items = menu.itemArray();
    for index in 0..items.count() {
        let item = items.objectAtIndex(index);
        let title = item.title().to_string();
        match delay_suffix_color(&title, &labels) {
            Some((start, tone)) => colorize_row(&item, &title, start, tone),
            None if item.attributedTitle().is_some() => item.setAttributedTitle(None),
            None => {}
        }
        if let Some(submenu) = item.submenu() {
            colorize_menu(&submenu, labels);
        }
    }
}

/// Rewrite `item`'s title with `tone` on the suffix that starts at byte
/// `start`, and leave the rest of the row alone.
///
/// The attributed title carries that one attribute on that one range: AppKit
/// draws the ranges without a colour in the menu's own text colour, so the
/// name reads the same as before in either appearance, and a highlighted row
/// still inverts it (an explicit colour would stick through the highlight).
/// No font attribute, either: the menu's own font is what an untouched row
/// gets, and setting one risks the baseline shift attributed titles are known
/// for.
#[cfg(target_os = "macos")]
fn colorize_row(item: &NSMenuItem, title: &str, start: usize, tone: DelayTone) {
    let (location, length) = suffix_units(title, start);
    let text = NSString::from_str(title);
    let attributed =
        NSMutableAttributedString::initWithString(NSMutableAttributedString::alloc(), &text);
    // SAFETY: the attribute name is AppKit's own, the value is the `NSColor`
    // AppKit documents for it, and the range lies inside the string.
    unsafe {
        attributed.addAttribute_value_range(
            NSForegroundColorAttributeName,
            &tone_color(tone),
            NSRange::new(location, length),
        );
    }
    item.setAttributedTitle(Some(&attributed));
}

/// Menu text colour for a result band. All three are dynamic system colours,
/// so they stay legible when the menu flips between the light and the dark
/// appearance.
#[cfg(target_os = "macos")]
fn tone_color(tone: DelayTone) -> Retained<NSColor> {
    match tone {
        // The Nodes page's green / yellow / red in system colours; the system
        // orange stands in for the system yellow, which barely reads against
        // the light menu background.
        DelayTone::Ok => NSColor::systemGreenColor(),
        DelayTone::Warn => NSColor::systemOrangeColor(),
        DelayTone::Bad => NSColor::systemRedColor(),
    }
}

/// Attach the delay buttons' custom views to the rows of the tray menu.
///
/// A click that picks a normal menu item closes the menu, and a close is how a
/// running delay test is cancelled (`tray_delay`): the click that starts a run
/// must not also end it. An item carrying a `view` takes the click itself and
/// the menu never selects it — but a rebuild drops the item and its view
/// together, so every rebuild attaches the views to its own fresh rows.
///
/// The walk goes over the menu rather than over the model: the model reached
/// the menu through muda, which keeps no handle on the native item a view
/// could be put on, while the menu itself is reachable through the status item.
/// Rows are found by position — the page button is the first row of the node
/// submenu, a group's button the first row of the group submenu — and a row is
/// only attached when its title still has the shape of the button it stands
/// for, so a menu built by another layout keeps its plain rows instead of
/// getting a view with the wrong scope. A group button's scope comes from the
/// model entry in the matching position: the menu and the model are laid out
/// by the same walk ([`append_node_entries`]), and a mismatch in their counts
/// skips the group buttons.
///
/// Off-main-thread only, like [`colorize_delay_results`]: the walk hops to the
/// main thread and blocks until it is done.
#[cfg(target_os = "macos")]
fn attach_delay_buttons(app: &AppHandle, entries: &[NodeMenuEntry], labels: &TrayLabels) {
    // The buttons are the ones the model carries: the top page's, then one per
    // group page, in the order `append_node_entries` lays them out. A body
    // without one (an empty page, a failed derivation) has nothing to attach.
    if !matches!(entries.first(), Some(NodeMenuEntry::Delay(_))) {
        return;
    }
    let expected = 1 + entries
        .iter()
        .filter(|entry| matches!(entry, NodeMenuEntry::Group(_)))
        .count();
    let scopes: Vec<DelayScope> = std::iter::once(DelayScope::Top)
        .chain(entries.iter().filter_map(|entry| match entry {
            NodeMenuEntry::Group(group) => Some(DelayScope::Group(group.tag.clone())),
            _ => None,
        }))
        .collect();
    let labels = *labels;
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let app = app.clone();
    let attached = tray.with_inner_tray_icon(move |tray| {
        let mtm = objc2::MainThreadMarker::new()?;
        let status_item = tray.ns_status_item()?;
        let menu = status_item.menu(mtm)?;
        Some(attach_delay_rows(&app, &menu, &scopes, &labels))
    });
    match attached {
        Ok(Some(attached)) if attached < expected => tracing::warn!(
            attached,
            expected,
            "tray delay buttons: rows did not match the node model"
        ),
        Ok(Some(_)) => {}
        Ok(None) => tracing::warn!("tray delay buttons: tray menu not found"),
        Err(err) => tracing::warn!(error = %err, "tray delay buttons: menu walk failed"),
    }
}

/// The menu half of [`attach_delay_buttons`]: the page button of the node
/// submenu, then the button of every group submenu under it. Returns how many
/// rows were given a view.
#[cfg(target_os = "macos")]
fn attach_delay_rows(
    app: &AppHandle,
    menu: &NSMenu,
    scopes: &[DelayScope],
    labels: &TrayLabels,
) -> usize {
    let Some(nodes) = submenu_titled(menu, labels.nodes) else {
        return 0;
    };
    let rows = nodes.itemArray();
    let mut attached = 0;
    // The node submenu opens with the page's own button, then its separator
    // and the rows below it (`append_node_entries`).
    if let Some(scope) = scopes.first() {
        if rows.count() > 0 {
            let row = rows.objectAtIndex(0);
            if attach_delay_row(app, &row, scope, labels) {
                attached += 1;
            }
        }
    }
    // A group's page button opens the group's submenu, and the model lists the
    // groups in the same order: pair them off position by position.
    let groups: Vec<Retained<NSMenuItem>> = (1..rows.count())
        .map(|index| rows.objectAtIndex(index))
        .filter(|row| row.submenu().is_some())
        .collect();
    let group_scopes: Vec<&DelayScope> = scopes.iter().skip(1).collect();
    if groups.len() != group_scopes.len() {
        tracing::warn!(
            menu = groups.len(),
            model = group_scopes.len(),
            "tray delay buttons: group pages do not match the node model"
        );
    }
    for (row, scope) in groups.iter().zip(group_scopes) {
        let Some(submenu) = row.submenu() else {
            continue;
        };
        let buttons = submenu.itemArray();
        if buttons.count() == 0 {
            continue;
        }
        let button = buttons.objectAtIndex(0);
        if attach_delay_row(app, &button, scope, labels) {
            attached += 1;
        }
    }
    attached
}

/// Put the row's custom view on it, if the row still has the shape of a delay
/// button; `scope` is what the run it starts will probe.
///
/// The row's own action is dropped with the view: on a plain item it fires on
/// keyboard activation, which also closes the menu, and that close would
/// cancel the run the action had just started. With a view attached the click
/// is the way a run starts, and a click on it leaves the menu open.
#[cfg(target_os = "macos")]
fn attach_delay_row(
    app: &AppHandle,
    row: &NSMenuItem,
    scope: &DelayScope,
    labels: &TrayLabels,
) -> bool {
    let title = row.title().to_string();
    if !delay_button_title_matches(&title, labels) {
        tracing::warn!(%title, "tray delay buttons: unexpected button row, left alone");
        return false;
    }
    // SAFETY: dropping the action and the target of this item's own native
    // menu item; AppKit keeps the item in its menu either way.
    unsafe {
        row.setAction(None);
        row.setTarget(None);
    }
    // A row that already carries one of our views is retitled through it: the
    // refresh then leaves the item — and the subview inside it — as the open
    // menu laid it out, and only the text it draws moves.
    if let Some(view) = row.view() {
        if crate::tray_delay_view::retitle_button_view(view, &title, row.isEnabled()) {
            return true;
        }
    }
    let Some(view) =
        crate::tray_delay_view::delay_button_view(app, &title, row.isEnabled(), scope.clone())
    else {
        return false;
    };
    row.setView(Some(&view));
    true
}

/// The submenu a top-level item carries under `title` — the node submenu,
/// looked up the way the walk knows it: by the label the menu shows.
#[cfg(target_os = "macos")]
fn submenu_titled(menu: &NSMenu, title: &str) -> Option<Retained<NSMenu>> {
    let items = menu.itemArray();
    (0..items.count())
        .map(|index| items.objectAtIndex(index))
        .find(|item| item.title().to_string() == title)
        .and_then(|item| item.submenu())
}

/// Whether `title` has the shape of a delay button: the page's own label, or a
/// progress text while a test is in flight (the page under test shows the
/// progress, the other pages keep their label).
///
/// `title` is the row's title as muda wrote it to the item — the label with
/// its mnemonics resolved (`menu_text`) — which is the label itself here: none
/// of the button labels carries an `&`.
///
/// Every page carries the same label, so the title alone cannot tell a page's
/// button from another's; the caller knows which page the row it walks belongs
/// to.
#[cfg(any(target_os = "macos", test))]
fn delay_button_title_matches(title: &str, labels: &TrayLabels) -> bool {
    title == labels.delay_test || delay_progress_shaped(title, labels)
}

/// Whether `title` is the progress text of `labels.delay_progress` — `延迟测试
/// 中 3/8` — told by shape: the template's own text around its two `{}`, and a
/// number in each of their places.
#[cfg(any(target_os = "macos", test))]
fn delay_progress_shaped(title: &str, labels: &TrayLabels) -> bool {
    let Some((prefix, rest)) = labels.delay_progress.split_once("{}") else {
        return false;
    };
    let Some((separator, suffix)) = rest.split_once("{}") else {
        return false;
    };
    let Some(numbers) = title
        .strip_prefix(prefix)
        .and_then(|rest| rest.strip_suffix(suffix))
    else {
        return false;
    };
    let Some((index, total)) = numbers.split_once(separator) else {
        return false;
    };
    let is_number = |text: &str| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    is_number(index) && is_number(total)
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
    // Cheap when settled: the prompt only touches the menu when the version
    // the window offers or the tray language moved.
    sync_update_prompt(app);
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

/// Bring the update prompt up to date on its own, for callers that changed
/// something the prompt renders (the tray language) and should not wait out
/// `SYNC_INTERVAL` for the watchdog. Cheap when settled.
///
/// Off-main-thread only, like [`sync_menu`]: the reconcile blocks on
/// main-thread menu mutations.
pub fn sync_update_prompt(app: &AppHandle) {
    let Some(menu) = app.try_state::<TrayMenuState>() else {
        return;
    };
    if let Err(err) = menu.sync_update() {
        tracing::warn!(
            code = %err.code,
            error = %err.message,
            "tray update prompt sync failed"
        );
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

/// Tray copy: same one-line Mixed command as the Home proxy-status card.
fn on_copy_cli_proxy(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        if let Err(err) = copy_proxy_terminal_command(state.inner()) {
            tracing::warn!(
                code = %err.code,
                error = %err.message,
                "tray copy proxy command failed"
            );
        }
    });
}

/// Tray open: same session-only Mixed terminal as the Home proxy-status card.
fn on_open_cli_proxy(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        if let Err(err) = open_proxy_terminal_from_state(state.inner()) {
            tracing::warn!(
                code = %err.code,
                error = %err.message,
                "tray open proxy terminal failed"
            );
        }
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

/// Show (`Some`) or drop (`None`) the menu's update prompt. The window reports
/// what the sidebar arrow shows: the version a background check found, or
/// nothing while automatic checks are off.
///
/// Off-main-thread only, like the other menu rebuilds: attaching the item
/// blocks on main-thread menu mutations.
pub fn set_update_available(app: &AppHandle, version: Option<String>) -> Result<(), AppError> {
    let state = app
        .try_state::<TrayMenuState>()
        .ok_or_else(|| AppError::new(ErrorCode::ConfigInvalid, "tray menu state is unavailable"))?;
    let wanted = version
        .map(|version| crate::app_update::normalize_version(&version).to_string())
        .filter(|version| !version.is_empty());
    state.apply_update(wanted)
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
        assert_eq!(
            (zh.copy_cli_proxy, zh.open_cli_proxy),
            ("复制代理命令", "打开代理终端")
        );
        assert_eq!((zh.show, zh.quit), ("显示", "退出"));
        assert_eq!(
            (zh.delay_test, zh.delay_progress, zh.delay_failed),
            ("延迟测试", "延迟测试中 {}/{}", "失败")
        );
        assert_eq!(zh.delay_progress_text(3, 8), "延迟测试中 3/8");

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
        assert_eq!(
            (en.copy_cli_proxy, en.open_cli_proxy),
            ("Copy Proxy Command", "Open Proxy Terminal")
        );
        assert_eq!((en.show, en.quit), ("Show", "Quit"));
        assert_eq!(
            (en.delay_test, en.delay_progress, en.delay_failed),
            ("Test Delay", "Testing {}/{}", "Failed")
        );
        assert_eq!(en.delay_progress_text(3, 8), "Testing 3/8");
    }

    #[test]
    fn update_prompt_text_names_the_version_per_language() {
        assert_eq!(
            update_prompt_text(TrayLanguage::Zh, "0.1.11"),
            "有新版本 0.1.11"
        );
        assert_eq!(
            update_prompt_text(TrayLanguage::En, "0.1.11"),
            "Version 0.1.11 is available"
        );
    }

    #[test]
    fn update_arrow_covers_head_and_shaft_only() {
        assert!(arrow_covers(0.5, 0.1)); // head, near the apex
        assert!(arrow_covers(0.2, 0.45)); // head, beside the shaft
        assert!(arrow_covers(0.5, 0.8)); // shaft
        assert!(!arrow_covers(0.5, 0.02)); // above the apex
        assert!(!arrow_covers(0.5, 0.99)); // below the shaft
        assert!(!arrow_covers(0.2, 0.9)); // beside the shaft
        assert!(!arrow_covers(0.02, 0.02)); // canvas corner
    }

    #[test]
    fn update_arrow_icon_is_green_on_transparency() {
        let icon = update_arrow_icon();
        assert_eq!(
            (icon.width(), icon.height()),
            (UPDATE_ARROW_SIZE, UPDATE_ARROW_SIZE)
        );
        let rgba = icon.rgba();
        assert_eq!(
            rgba.len(),
            (UPDATE_ARROW_SIZE * UPDATE_ARROW_SIZE * 4) as usize
        );
        let pixel = |x: u32, y: u32| {
            let offset = ((y * UPDATE_ARROW_SIZE + x) * 4) as usize;
            &rgba[offset..offset + 4]
        };
        // Outside the arrow the menu background shows through.
        assert_eq!(pixel(0, 0)[3], 0);
        assert_eq!(pixel(UPDATE_ARROW_SIZE - 1, 0)[3], 0);
        // The shaft carries the sidebar arrow's green.
        let shaft = pixel(UPDATE_ARROW_SIZE / 2, UPDATE_ARROW_SIZE - 3);
        assert_eq!(&shaft[..3], &UPDATE_ARROW_GREEN);
        assert_eq!(shaft[3], 255);
    }

    #[test]
    fn update_prompt_step_tracks_attach_retext_and_detach() {
        assert_eq!(update_prompt_step(None, None), UpdatePromptStep::Settled);
        assert_eq!(
            update_prompt_step(Some("a"), Some("a")),
            UpdatePromptStep::Settled
        );
        assert_eq!(
            update_prompt_step(Some("a"), None),
            UpdatePromptStep::Attach
        );
        assert_eq!(
            update_prompt_step(Some("a"), Some("b")),
            UpdatePromptStep::Retext
        );
        assert_eq!(
            update_prompt_step(None, Some("a")),
            UpdatePromptStep::Detach
        );
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
        for id in [
            "service",
            "show",
            "quit",
            "copy-cli-proxy",
            "open-cli-proxy",
            "[\"group\",0]",
            "[\"member\"]",
        ] {
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

    /// A delay view with a run and the outcomes of a finished or running test.
    fn delay_view(
        run: Option<(DelayScope, usize, usize)>,
        outcomes: &[(&str, DelayOutcome)],
    ) -> DelayView {
        DelayView {
            run,
            outcomes: outcomes
                .iter()
                .map(|(tag, outcome)| (tag.to_string(), *outcome))
                .collect(),
        }
    }

    fn delay_button_of(entry: &NodeMenuEntry) -> &DelayButton {
        match entry {
            NodeMenuEntry::Delay(button) => button,
            other => panic!("expected a delay button, got {other:?}"),
        }
    }

    #[test]
    fn delay_decoration_puts_a_button_on_every_page() {
        let nodes = vec![
            node(
                "节点选择",
                "selector",
                Some("日本 02"),
                Some(&["香港 01", "日本 02"]),
            ),
            node("香港 01", "socks", None, None),
            node("日本 02", "vmess", None, None),
        ];
        let mut entries = node_menu_entries(&nodes, "香港 01");
        apply_delay(
            &mut entries,
            &labels(TrayLanguage::Zh),
            &nodes,
            &delay_view(None, &[]),
        );

        // Top page: the button above the group, labels untouched while idle.
        let top = delay_button_of(&entries[0]);
        assert_eq!(top.label, "延迟测试");
        assert!(top.enabled);
        assert_eq!(top.scope, DelayScope::Top);
        let group = group(&entries[1]);
        assert_eq!(group.label, "节点选择 → 日本 02");
        let page = group.delay.as_ref().expect("group page button");
        assert_eq!(page.label, "延迟测试");
        assert!(page.enabled);
        assert_eq!(page.scope, DelayScope::Group("节点选择".into()));
    }

    #[test]
    fn delay_decoration_appends_the_newest_result_to_its_rows() {
        let nodes = vec![
            node(
                "节点选择",
                "selector",
                Some("日本 02"),
                Some(&["香港 01", "日本 02"]),
            ),
            node("香港 01", "socks", None, None),
            node("日本 02", "vmess", None, None),
        ];
        let view = delay_view(
            Some((DelayScope::Top, 2, 2)),
            &[
                ("香港 01", DelayOutcome::Done(45)),
                ("日本 02", DelayOutcome::Testing),
            ],
        );
        let mut entries = node_menu_entries(&nodes, "香港 01");
        apply_delay(&mut entries, &labels(TrayLanguage::Zh), &nodes, &view);

        // The button of the page under test shows the progress, disabled…
        let top = delay_button_of(&entries[0]);
        assert_eq!(top.label, "延迟测试中 2/2");
        assert!(!top.enabled);
        // …the group label mirrors the result of the member it exits through…
        let group = group(&entries[1]);
        assert_eq!(group.label, "节点选择 → 日本 02 · …");
        let page = group.delay.as_ref().expect("group page button");
        assert_eq!(page.label, "延迟测试");
        assert!(!page.enabled);
        // …and every member carries its own result.
        assert_eq!(group.members[0].label, "香港 01 · 45 ms");
        assert_eq!(group.members[1].label, "日本 02 · …");
    }

    #[test]
    fn delay_decoration_names_a_failed_probe_per_language() {
        let nodes = vec![
            node(
                "节点选择",
                "selector",
                Some("日本 02"),
                Some(&["香港 01", "日本 02"]),
            ),
            node("香港 01", "socks", None, None),
            node("日本 02", "vmess", None, None),
        ];
        let view = delay_view(
            Some((DelayScope::Group("节点选择".into()), 1, 2)),
            &[
                ("香港 01", DelayOutcome::Failed),
                ("日本 02", DelayOutcome::Testing),
            ],
        );
        for (language, failed, group_label) in [
            (TrayLanguage::Zh, "香港 01 · 失败", "节点选择 → 日本 02 · …"),
            (
                TrayLanguage::En,
                "香港 01 · Failed",
                "节点选择 → 日本 02 · …",
            ),
        ] {
            let mut entries = node_menu_entries(&nodes, "香港 01");
            apply_delay(&mut entries, &labels(language), &nodes, &view);
            let group = group(&entries[1]);
            assert_eq!(group.members[0].label, failed);
            assert_eq!(group.label, group_label);
            // A run on another page leaves the top button disabled with its
            // plain label.
            let top = delay_button_of(&entries[0]);
            assert_eq!(top.label, labels(language).delay_test);
            assert!(!top.enabled);
            // The group page's own button carries the progress.
            let page = group.delay.as_ref().expect("group page button");
            assert_eq!(page.label, labels(language).delay_progress_text(1, 2));
        }
    }

    #[test]
    fn delay_suffix_color_locates_the_finished_results() {
        let zh = labels(TrayLanguage::Zh);
        let name = "香港 01";
        // Every band, on a plain row and on a group row that carries the
        // result of the member it exits through.
        for (delay_ms, tone) in [
            (45, DelayTone::Ok),
            (299, DelayTone::Ok),
            (300, DelayTone::Warn),
            (999, DelayTone::Warn),
            (1000, DelayTone::Bad),
        ] {
            for title in [
                format!("{name}{SUFFIX_SEPARATOR}{delay_ms}{DELAY_UNIT}"),
                format!("节点选择 → 日本 02{SUFFIX_SEPARATOR}{delay_ms}{DELAY_UNIT}"),
            ] {
                let start = title.rfind(SUFFIX_SEPARATOR).expect("separator");
                assert_eq!(
                    delay_suffix_color(&title, &zh),
                    Some((start, tone)),
                    "{title}"
                );
            }
        }
        // The suffix is what is coloured, separator included: a name that held
        // the separator itself keeps its own bytes untouched.
        let title = format!("A{SUFFIX_SEPARATOR}B{SUFFIX_SEPARATOR}45{DELAY_UNIT}");
        let (start, tone) = delay_suffix_color(&title, &zh).expect("finished result");
        assert_eq!(tone, DelayTone::Ok);
        assert_eq!(&title[start..], format!("{SUFFIX_SEPARATOR}45{DELAY_UNIT}"));
    }

    #[test]
    fn delay_suffix_color_keeps_to_finished_results() {
        let zh = labels(TrayLanguage::Zh);
        let en = labels(TrayLanguage::En);
        // A probe in flight prints `…`, and a row the test never reached is
        // bare: neither asks for a colour.
        assert_eq!(
            delay_suffix_color(&format!("香港 01{SUFFIX_SEPARATOR}…"), &zh),
            None
        );
        assert_eq!(delay_suffix_color("香港 01", &zh), None);
        // A failure is coloured per the menu's own language…
        for language in [TrayLanguage::Zh, TrayLanguage::En] {
            let labels = labels(language);
            let title = format!("香港 01{SUFFIX_SEPARATOR}{}", labels.delay_failed);
            assert_eq!(
                delay_suffix_color(&title, &labels),
                Some(("香港 01".len(), DelayTone::Bad)),
                "{title}"
            );
        }
        // …and another language's label is not this menu's.
        assert_eq!(
            delay_suffix_color(
                &format!("香港 01{SUFFIX_SEPARATOR}{}", en.delay_failed),
                &zh
            ),
            None
        );
    }

    #[test]
    fn suffix_units_count_utf16_code_units_not_bytes() {
        // 日 and 本 are three bytes each, the emoji four — and two UTF-16 units.
        let title = format!("日本😀{SUFFIX_SEPARATOR}45{DELAY_UNIT}");
        let start = title.rfind(SUFFIX_SEPARATOR).expect("separator");
        assert_eq!(start, 10);
        assert_eq!(
            delay_suffix_color(&title, &labels(TrayLanguage::Zh)),
            Some((10, DelayTone::Ok))
        );
        // What AppKit needs: the location in UTF-16 units, the suffix's own
        // length in the same units.
        assert_eq!(suffix_units(&title, start), (4, 8));
    }

    #[test]
    fn a_flat_profile_gets_the_top_button_above_its_nodes() {
        let nodes = vec![
            node("香港 01", "socks", None, None),
            node("日本 02", "vmess", None, None),
        ];
        let view = delay_view(None, &[("香港 01", DelayOutcome::Done(12))]);
        let mut entries = node_menu_entries(&nodes, "香港 01");
        apply_delay(&mut entries, &labels(TrayLanguage::En), &nodes, &view);
        assert_eq!(entries.len(), 3);
        let top = delay_button_of(&entries[0]);
        assert_eq!(top.label, "Test Delay");
        assert!(top.enabled);
        assert_eq!(item(&entries[1]).label, "香港 01 · 12 ms");
        // A node the newest test did not probe keeps its bare label.
        assert_eq!(item(&entries[2]).label, "日本 02");
    }

    #[test]
    fn a_page_without_rows_gets_no_button() {
        let mut entries: Vec<NodeMenuEntry> = Vec::new();
        apply_delay(
            &mut entries,
            &labels(TrayLanguage::Zh),
            &[],
            &delay_view(None, &[]),
        );
        assert!(entries.is_empty());
    }

    #[test]
    fn delay_button_titles_are_recognised_by_shape() {
        let zh = labels(TrayLanguage::Zh);
        // The plain label both page kinds carry, and the progress text a run
        // prints on the page it is testing…
        assert!(delay_button_title_matches("延迟测试", &zh));
        assert!(delay_button_title_matches("延迟测试中 3/8", &zh));
        assert!(delay_button_title_matches("延迟测试中 12/100", &zh));
        // …and nothing else: a row, a half-written progress text, another
        // language's label, or a leftover of the old wording.
        assert!(!delay_button_title_matches("香港 01", &zh));
        assert!(!delay_button_title_matches("延迟测试中 3/", &zh));
        assert!(!delay_button_title_matches("延迟测试中 /8", &zh));
        assert!(!delay_button_title_matches("延迟测试中 3/x", &zh));
        assert!(!delay_button_title_matches("测试中 3/8", &zh));
        assert!(!delay_button_title_matches("测速：当前出口", &zh));
        assert!(!delay_button_title_matches("Testing 3/8", &zh));

        let en = labels(TrayLanguage::En);
        assert!(delay_button_title_matches("Test Delay", &en));
        assert!(delay_button_title_matches("Testing 3/8", &en));
    }

    #[test]
    fn a_retitle_keeps_the_rows_it_rewrites() {
        let zh = labels(TrayLanguage::Zh);
        let nodes = vec![
            node(
                "节点选择",
                "selector",
                Some("日本 02"),
                Some(&["香港 01", "日本 02"]),
            ),
            node("香港 01", "socks", None, None),
            node("日本 02", "vmess", None, None),
        ];
        let mut idle = node_menu_entries(&nodes, "香港 01");
        apply_delay(&mut idle, &zh, &nodes, &delay_view(None, &[]));

        // A run writes progress and results into the rows already on the menu…
        let mut running = node_menu_entries(&nodes, "香港 01");
        apply_delay(
            &mut running,
            &zh,
            &nodes,
            &delay_view(
                Some((DelayScope::Top, 1, 2)),
                &[("香港 01", DelayOutcome::Done(42))],
            ),
        );
        assert_ne!(idle, running, "the run moves the labels");
        assert!(same_node_rows(&idle, &running));

        // …and a pick moves the check mark on the same rows.
        let flat = vec![
            node("香港 01", "socks", None, None),
            node("日本 02", "vmess", None, None),
        ];
        let mut before = node_menu_entries(&flat, "香港 01");
        apply_delay(&mut before, &zh, &flat, &delay_view(None, &[]));
        let mut picked = node_menu_entries(&flat, "日本 02");
        apply_delay(&mut picked, &zh, &flat, &delay_view(None, &[]));
        assert_ne!(before, picked, "the pick moves the check mark");
        assert!(same_node_rows(&before, &picked));
    }

    #[test]
    fn a_different_node_list_is_not_a_retitle() {
        let zh = labels(TrayLanguage::Zh);
        let nodes = vec![
            node("香港 01", "socks", None, None),
            node("日本 02", "vmess", None, None),
        ];
        let mut listed = node_menu_entries(&nodes, "香港 01");
        apply_delay(&mut listed, &zh, &nodes, &delay_view(None, &[]));

        // A node left the profile: the model no longer has a row for the one
        // the submenu still holds.
        let fewer = vec![node("香港 01", "socks", None, None)];
        let mut shortened = node_menu_entries(&fewer, "香港 01");
        apply_delay(&mut shortened, &zh, &fewer, &delay_view(None, &[]));
        assert!(!same_node_rows(&listed, &shortened));

        // A group lost a member: its page is not the page on the menu either.
        let wide = vec![
            node(
                "节点选择",
                "selector",
                Some("日本 02"),
                Some(&["香港 01", "日本 02"]),
            ),
            node("香港 01", "socks", None, None),
            node("日本 02", "vmess", None, None),
        ];
        let mut grouped = node_menu_entries(&wide, "香港 01");
        apply_delay(&mut grouped, &zh, &wide, &delay_view(None, &[]));
        let narrow = vec![
            node("节点选择", "selector", Some("香港 01"), Some(&["香港 01"])),
            node("香港 01", "socks", None, None),
            node("日本 02", "vmess", None, None),
        ];
        let mut thinned = node_menu_entries(&narrow, "香港 01");
        apply_delay(&mut thinned, &zh, &narrow, &delay_view(None, &[]));
        assert!(!same_node_rows(&grouped, &thinned));
    }
}
