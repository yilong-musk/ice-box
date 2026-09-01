//! System tray: close → hide; Quit → Stop then exit.

use crate::shutdown::{request_tray_quit, QuitOutcome};
use ice_config::{AppError, ErrorCode, LanguagePreference};
use serde::Deserialize;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, Runtime,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrayLanguage {
    Zh,
    En,
}

impl From<LanguagePreference> for TrayLanguage {
    fn from(preference: LanguagePreference) -> Self {
        match preference {
            LanguagePreference::Zh => Self::Zh,
            LanguagePreference::System | LanguagePreference::En => Self::En,
        }
    }
}

fn labels(language: TrayLanguage) -> (&'static str, &'static str) {
    match language {
        TrayLanguage::Zh => ("显示", "退出"),
        TrayLanguage::En => ("Show", "Quit"),
    }
}

struct TrayMenuState<R: Runtime> {
    show: MenuItem<R>,
    quit: MenuItem<R>,
}

pub fn setup_tray<R: Runtime>(app: &AppHandle<R>, language: TrayLanguage) -> tauri::Result<()> {
    let (show_label, quit_label) = labels(language);
    let show = MenuItem::with_id(app, "show", show_label, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", quit_label, true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    app.manage(TrayMenuState {
        show: show.clone(),
        quit: quit.clone(),
    });

    let mut builder = TrayIconBuilder::new()
        .menu(&menu)
        .tooltip("ice-box")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.show();
                    let _ = win.set_focus();
                }
            }
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
                let app = tray.app_handle();
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.show();
                    let _ = win.set_focus();
                }
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    let _tray = builder.build(app)?;
    Ok(())
}

pub fn set_language<R: Runtime>(
    app: &AppHandle<R>,
    language: TrayLanguage,
) -> Result<(), AppError> {
    let state = app
        .try_state::<TrayMenuState<R>>()
        .ok_or_else(|| AppError::new(ErrorCode::ConfigInvalid, "tray menu state is unavailable"))?;
    let (show, quit) = labels(language);
    state.show.set_text(show).map_err(|err| {
        AppError::new(
            ErrorCode::ConfigInvalid,
            format!("update tray Show label: {err}"),
        )
    })?;
    state.quit.set_text(quit).map_err(|err| {
        AppError::new(
            ErrorCode::ConfigInvalid,
            format!("update tray Quit label: {err}"),
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_labels_cover_supported_languages() {
        assert_eq!(labels(TrayLanguage::Zh), ("显示", "退出"));
        assert_eq!(labels(TrayLanguage::En), ("Show", "Quit"));
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
