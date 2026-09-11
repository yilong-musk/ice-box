// SPDX-License-Identifier: GPL-3.0-or-later

//! System tray: close → hide; left click → show; Quit → Stop then exit.
//!
//! The menu also carries the two actions that do not need the window: the proxy
//! service switch (labeled with the action, like the Home power button) and the
//! routing mode group. A watchdog re-derives both from the runtime state, so
//! the menu follows changes made anywhere else — window, recovery, or a manual
//! OS edit.

use crate::capture::TrafficCapture;
use crate::commands::{
    apply_proxy_mode, current_settings, disable_active_backend_inner, proxy_service_posture,
    start_service,
};
use crate::shutdown::{request_tray_quit, QuitOutcome};
use crate::AppState;
use ice_config::{AppError, ErrorCode, LanguagePreference, ProxyMode};
use ice_core::CoreStatus;
use serde::Deserialize;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::Duration;
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu},
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
            show: "Show",
            quit: "Quit",
        },
    }
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
        result.map_err(|err| {
            AppError::new(
                ErrorCode::ConfigInvalid,
                format!("update tray {what} label: {err}"),
            )
        })
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
        self.update_label("Show", self.show.set_text(labels.show))?;
        self.update_label("Quit", self.quit.set_text(labels.quit))?;
        self.language.store(language.code(), Ordering::SeqCst);
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
    let show = MenuItem::with_id(app, "show", labels.show, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", labels.quit, true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&service, &mode, &separator, &show, &quit])?;
    app.manage(TrayMenuState {
        service,
        mode,
        mode_rule,
        mode_global,
        mode_direct,
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
        .on_menu_event(|app, event| match event.id.as_ref() {
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

/// Re-derive the menu from the runtime state. Cheap enough for the watchdog:
/// one settings read, one proxy record read, and the memoized OS probe.
pub fn sync_menu(app: &AppHandle) {
    let Some(menu) = app.try_state::<TrayMenuState>() else {
        return;
    };
    let Some(view) = current_view(app) else {
        return;
    };
    menu.apply_view(view);
}

/// Keep the service switch and the mode group in step with state changes the
/// tray did not make: window actions, recovery, and external OS edits.
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
        sync_menu(&app);
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
        sync_menu(&app);
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
        assert_eq!((zh.show, zh.quit), ("显示", "退出"));

        let en = labels(TrayLanguage::En);
        assert_eq!(en.service(false), "Start Proxy Service");
        assert_eq!(en.service(true), "Stop Proxy Service");
        assert_eq!(en.mode, "Proxy Mode");
        assert_eq!(
            (en.mode_rule, en.mode_global, en.mode_direct),
            ("Rule", "Global", "Direct")
        );
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
