// SPDX-License-Identifier: GPL-3.0-or-later

//! Windows tray compatibility for third-party taskbar / dock hosts (such as
//! MyDockFinder).
//!
//! `tray-icon` queries `Shell_NotifyIconGetRect` before emitting a click event
//! and drops the message when the query fails. Third-party tray hosts forward
//! mouse clicks without implementing that rect query (or send `WM_CONTEXTMENU`
//! directly), which prevents the tray panel from opening.
//!
//! This module subclasses the tray icon's hidden message window: when Explorer
//! answers `Shell_NotifyIconGetRect`, messages pass straight through to
//! `tray-icon`; otherwise the current cursor position stands in for the icon
//! rect and dispatches through the same tray click handler.

use crate::tray::TRAY_ID;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
use tauri::{AppHandle, PhysicalPosition, PhysicalSize, Rect};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, S_OK, WPARAM};
use windows_sys::Win32::UI::Shell::{
    DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass, Shell_NotifyIconGetRect,
    NOTIFYICONIDENTIFIER,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, WM_CONTEXTMENU, WM_LBUTTONUP, WM_NCDESTROY, WM_RBUTTONUP,
};

/// Callback message `tray-icon` registers (`WM_USER_TRAYICON`, internal to the
/// `tray-icon` crate on Windows; verify if upgrading `tray-icon`).
const TRAY_CALLBACK_MSG: u32 = 6002;
/// Unique subclass id for ice-box (`"ICEB"`).
const SUBCLASS_ID: usize = 0x4943_4542;
/// Deduplicate a right click that arrives as both `WM_RBUTTONUP` and `WM_CONTEXTMENU`.
const RIGHT_CLICK_DEDUP: Duration = Duration::from_millis(300);

static APP: OnceLock<AppHandle> = OnceLock::new();
static LAST_RIGHT_CLICK: Mutex<Option<Instant>> = Mutex::new(None);

/// Attach the tray subclass after the tray icon is built (UI thread).
pub fn install(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let hwnd = match tray.with_inner_tray_icon(|inner| inner.window_handle() as isize) {
        Ok(hwnd) => hwnd,
        Err(err) => {
            tracing::warn!(error = %err, "tray compat: failed to read tray window handle");
            return;
        }
    };
    let _ = APP.set(app.clone());
    // SAFETY: `hwnd` is the tray's live message-only window on the UI thread,
    // and `subclass_proc` matches `SUBCLASSPROC`.
    if unsafe { SetWindowSubclass(hwnd as HWND, Some(subclass_proc), SUBCLASS_ID, 0) } == 0 {
        tracing::warn!(
            error = %std::io::Error::last_os_error(),
            "tray compat: SetWindowSubclass failed"
        );
    }
}

unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    id: usize,
    _data: usize,
) -> LRESULT {
    match msg {
        WM_NCDESTROY => {
            // SAFETY: Win32 `SetWindowSubclass` contract requires removing
            // the subclass before the window finishes destruction.
            RemoveWindowSubclass(hwnd, Some(subclass_proc), id);
        }
        TRAY_CALLBACK_MSG => {
            // Mask to LOWORD so both legacy and `NOTIFYICON_VERSION_4`
            // callbacks match the mouse message cleanly.
            let mouse_msg = (lparam as u32) & 0xffff;
            if matches!(mouse_msg, WM_LBUTTONUP | WM_RBUTTONUP) {
                let button = if mouse_msg == WM_LBUTTONUP {
                    MouseButton::Left
                } else {
                    MouseButton::Right
                };
                if icon_rect_available(hwnd, wparam as u32) {
                    if button == MouseButton::Right {
                        claim_right_click();
                    }
                } else {
                    if button == MouseButton::Left || claim_right_click() {
                        post_click(button);
                    }
                    return 0;
                }
            }
        }
        WM_CONTEXTMENU => {
            if claim_right_click() {
                post_click(MouseButton::Right);
            }
            return 0;
        }
        _ => {}
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

fn icon_rect_available(hwnd: HWND, id: u32) -> bool {
    // SAFETY: `NOTIFYICONIDENTIFIER` is a POD C struct; zero-init leaves
    // `guidItem` empty so Win32 identifies the icon by `(hWnd, uID)`.
    let mut nid: NOTIFYICONIDENTIFIER = unsafe { std::mem::zeroed() };
    nid.cbSize = std::mem::size_of::<NOTIFYICONIDENTIFIER>() as u32;
    nid.hWnd = hwnd;
    nid.uID = id;
    let mut rect: RECT = unsafe { std::mem::zeroed() };
    // SAFETY: both pointers point to initialized stack structs.
    unsafe { Shell_NotifyIconGetRect(&nid, &mut rect) == S_OK }
}

fn claim_right_click() -> bool {
    let mut last = LAST_RIGHT_CLICK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let duplicate = last.is_some_and(|at| at.elapsed() < RIGHT_CLICK_DEDUP);
    *last = Some(Instant::now());
    !duplicate
}

fn post_click(button: MouseButton) {
    let Some(app) = APP.get().cloned() else {
        return;
    };
    let mut cursor = POINT { x: 0, y: 0 };
    // SAFETY: `cursor` is a valid mutable stack struct.
    unsafe { GetCursorPos(&mut cursor) };
    let (x, y) = (cursor.x, cursor.y);
    tauri::async_runtime::spawn(async move {
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || dispatch_click(&handle, button, x, y));
    });
}

fn dispatch_click(app: &AppHandle, button: MouseButton, x: i32, y: i32) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let rect = Rect {
        position: PhysicalPosition::new(x, y).into(),
        size: PhysicalSize::new(1u32, 1u32).into(),
    };
    crate::tray::handle_tray_icon_event(
        &tray,
        TrayIconEvent::Click {
            id: tray.id().clone(),
            position: PhysicalPosition::new(x as f64, y as f64),
            rect,
            button,
            button_state: MouseButtonState::Up,
        },
    );
}
