// SPDX-License-Identifier: GPL-3.0-or-later

//! Custom Windows system tray panel: a borderless transparent WebView2 popup
//! styled to match the macOS tray menu.
//!
//! Right-clicking the Windows tray icon opens a lazy-created WebView window
//! anchored beside the icon. To avoid DWM resize flicker when submenus
//! (Proxy Mode, Nodes, Strategy Group members, Subscriptions) expand and
//! collapse, the window keeps a fixed transparent canvas sized for all three
//! cascading columns during a popup session. Each visible menu card paints its
//! own opaque surface, and Win32 `CreateRoundRectRgn` + `CombineRgn(RGN_OR)` +
//! `SetWindowRgn` clips the HWND strictly to the union of the visible cards so
//! unused canvas area stays transparent and click-through.
//!
//! Actions reuse the same menu ids as the native menu (`tray::handle_panel_action`).
//! Clicking a delay-test row keeps the panel open so live results stream in;
//! dismissing the panel cancels any in-flight run and drops its results,
//! matching the macOS tray menu behaviour.
//!
//! **Cross-platform parity**: Keep this Windows panel and its frontend
//! (`apps/desktop/src/tray-panel/TrayPanel.tsx`, `tray-panel.css`) visually
//! and structurally in sync with the macOS native tray menu (`tray.rs`,
//! `tray_delay_view.rs`). Updating tray menu style or structure on one
//! platform requires updating the other in the same change.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

pub const PANEL_LABEL: &str = "tray-panel";

/// Sent to the panel before reveal with the popup placement in logical pixels.
#[cfg(target_os = "windows")]
const SHOW_EVENT: &str = "tray-panel://show";
/// Sent when the panel hides so the frontend clears open submenu state.
const HIDE_EVENT: &str = "tray-panel://hide";
/// Sent when runtime state, delay results, or settings change while open.
const CHANGED_EVENT: &str = "tray-panel://changed";

/// Default column widths in logical pixels.
#[cfg(any(target_os = "windows", test))]
const MAIN_WIDTH: f64 = 228.0;
#[cfg(any(target_os = "windows", test))]
const MODE_WIDTH: f64 = 152.0;
#[cfg(any(target_os = "windows", test))]
const SUBS_WIDTH: f64 = 228.0;
#[cfg(any(target_os = "windows", test))]
const SUB_WIDTH: f64 = 272.0;
#[cfg(any(target_os = "windows", test))]
const LEAF_WIDTH: f64 = 264.0;
/// Horizontal gap between cascading menu cards (logical pixels).
#[cfg(any(target_os = "windows", test))]
const CARD_GAP: f64 = 4.0;
/// Card corner radius in logical pixels; matches CSS `.tp-card` border-radius.
#[cfg(any(target_os = "windows", test))]
const CARD_RADIUS: f64 = 10.0;
/// Distance between the tray icon edge and the main menu card (logical pixels).
#[cfg(any(target_os = "windows", test))]
const ICON_GAP: f64 = 6.0;
/// Minimum margin from the monitor work-area edges (logical pixels).
#[cfg(any(target_os = "windows", test))]
const SCREEN_MARGIN: f64 = 8.0;
/// Grace window after blur-hide: clicking the tray icon to dismiss fires
/// `Focused(false)` before `TrayIconEvent::Click`, so a click inside this
/// window completes the dismiss instead of reopening the panel.
#[cfg(target_os = "windows")]
const BLUR_CLICK_GRACE: Duration = Duration::from_millis(300);
/// Coalesce bursts of state/delay notifications while the panel is visible.
const CHANGED_THROTTLE: Duration = Duration::from_millis(120);

/// Placement sent to the panel frontend for a single popup session.
/// All coordinates and dimensions are in native logical pixels.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelPlacement {
    /// Monotonic popup sequence number; stale layout reports are ignored.
    pub seq: u64,
    /// DPI scale factor of the monitor holding the tray icon.
    pub scale_factor: f64,
    /// Horizontal direction submenus expand toward (`"left"` or `"right"`).
    pub side: &'static str,
    /// Main card edge anchored to the tray icon (`"top"` or `"bottom"`).
    pub edge: &'static str,
    /// Work-area top/bottom bounds relative to the anchor `y` (negative upward).
    pub bounds_top: f64,
    pub bounds_bottom: f64,
    pub main_width: f64,
    pub mode_width: f64,
    pub subs_width: f64,
    pub sub_width: f64,
    pub leaf_width: f64,
    pub gap: f64,
    pub radius: f64,
}

#[derive(Debug, Clone)]
struct Anchor {
    /// Screen X of the main card's left edge (logical pixels).
    x: f64,
    /// Screen Y of the main card's anchored edge (logical pixels).
    y: f64,
    /// Monitor scale factor for converting logical coordinates to physical pixels.
    scale: f64,
    placement: PanelPlacement,
}

#[cfg(any(target_os = "windows", test))]
#[derive(Debug, Clone, Copy)]
struct Area {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

/// A visible menu card rectangle inside the panel canvas (logical pixels).
#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CardRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Window outer frame on screen (logical pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Frame {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

/// Integer physical-pixel rectangle used for Win32 frame and region calculations.
#[cfg(any(target_os = "windows", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PhysicalRect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

/// Layout reported back by the frontend after measuring visible cards.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelLayout {
    pub seq: u64,
    pub width: f64,
    pub height: f64,
    /// Main card left X inside the canvas (logical pixels).
    pub offset_x: f64,
    /// Canvas top Y relative to the anchor edge (logical pixels).
    pub offset_y: f64,
    pub cards: Vec<CardRect>,
    /// Explicit dark/light theme choice, or `None` when following the OS.
    pub dark: Option<bool>,
}

static ANCHOR: Mutex<Option<Anchor>> = Mutex::new(None);
static SEQ: AtomicU64 = AtomicU64::new(0);
static PENDING_REVEAL: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "windows")]
static LAST_BLUR_HIDE: Mutex<Option<Instant>> = Mutex::new(None);
static APPLIED_DARK: Mutex<Option<Option<bool>>> = Mutex::new(None);
static APPLIED_FRAME: Mutex<Option<Frame>> = Mutex::new(None);
static CHANGED_STATE: Mutex<(Option<Instant>, bool)> = Mutex::new((None, false));

fn lock_recover<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Compute anchor coordinates and three-column cascade placement for `icon`
/// inside `work` (both in the monitor's logical pixels).
#[cfg(any(target_os = "windows", test))]
fn place(
    icon: (f64, f64, f64, f64),
    work: Area,
    seq: u64,
    scale_factor: f64,
) -> (f64, f64, PanelPlacement) {
    let (ix, iy, iw, ih) = icon;
    let gap = CARD_GAP;
    let total_columns = MAIN_WIDTH + SUB_WIDTH + LEAF_WIDTH;
    let available_width = (work.right - work.left - SCREEN_MARGIN * 2.0 - gap * 2.0).max(1.0);
    let width_scale = (available_width / total_columns).min(1.0);
    let main_width = (MAIN_WIDTH * width_scale).round();
    let mode_width = (MODE_WIDTH * width_scale).round();
    let subs_width = (SUBS_WIDTH * width_scale).round();
    let sub_width = (SUB_WIDTH * width_scale).round();
    let leaf_width = (LEAF_WIDTH * width_scale).round();

    let min_x = work.left + SCREEN_MARGIN;
    let max_x = (work.right - SCREEN_MARGIN - main_width).max(min_x);
    let mut x = (ix + iw / 2.0 - main_width / 2.0).clamp(min_x, max_x);

    let upward = iy + ih / 2.0 > (work.top + work.bottom) / 2.0;
    let (edge, y) = if upward {
        ("bottom", (iy - ICON_GAP).min(work.bottom - SCREEN_MARGIN))
    } else {
        ("top", (iy + ih + ICON_GAP).max(work.top + SCREEN_MARGIN))
    };

    let cascade_width = sub_width + gap + leaf_width;
    let room_right = work.right - SCREEN_MARGIN - (x + main_width + gap);
    let room_left = x - gap - min_x;
    let side = if room_right >= cascade_width || room_right >= room_left {
        "right"
    } else {
        "left"
    };

    // Keep all three cascading columns inside the active monitor's work area.
    x = if side == "left" {
        x.max(min_x + cascade_width + gap).min(max_x)
    } else {
        x.min(work.right - SCREEN_MARGIN - main_width - gap - cascade_width)
            .max(min_x)
    };

    let placement = PanelPlacement {
        seq,
        scale_factor,
        side,
        edge,
        bounds_top: work.top + SCREEN_MARGIN - y,
        bounds_bottom: work.bottom - SCREEN_MARGIN - y,
        main_width,
        mode_width,
        subs_width,
        sub_width,
        leaf_width,
        gap,
        radius: CARD_RADIUS,
    };
    (x, y, placement)
}

/// Convert a desired client-area `frame` (logical pixels) into an outer window
/// rectangle (physical pixels), compensating for any invisible non-client frame
/// borders Windows keeps on borderless windows.
#[cfg(any(target_os = "windows", test))]
fn compute_outer_rect(
    frame: Frame,
    scale: f64,
    outer: PhysicalRect,
    client: PhysicalRect,
    origin: (i32, i32),
) -> PhysicalRect {
    let left_border = origin.0 - outer.left;
    let top_border = origin.1 - outer.top;
    let right_border = outer.right - origin.0 - (client.right - client.left);
    let bottom_border = outer.bottom - origin.1 - (client.bottom - client.top);
    let x = (frame.x * scale).round() as i32;
    let y = (frame.y * scale).round() as i32;
    PhysicalRect {
        left: x - left_border,
        top: y - top_border,
        right: x + (frame.width * scale).round() as i32 + right_border,
        bottom: y + (frame.height * scale).round() as i32 + bottom_border,
    }
}

/// Physical top-left point inside the target monitor's work area used to move
/// the hidden panel onto that monitor before the frontend measures layout, so
/// mixed-DPI setups receive `WM_DPICHANGED` up front.
#[cfg(any(target_os = "windows", test))]
fn preposition_origin(work: Area, scale: f64) -> (i32, i32) {
    (
        ((work.left + SCREEN_MARGIN) * scale).round() as i32,
        ((work.top + SCREEN_MARGIN) * scale).round() as i32,
    )
}

/// Convert visible logical `cards` into physical GDI region rectangles offset
/// from the window's outer top-left corner. Win32 `CreateRoundRectRgn` treats
/// `right` and `bottom` as exclusive, so each edge adds `+1`.
#[cfg(any(target_os = "windows", test))]
fn card_region_rects(
    cards: &[CardRect],
    scale: f64,
    client_offset: (i32, i32),
) -> Vec<PhysicalRect> {
    let (ox, oy) = client_offset;
    cards
        .iter()
        .filter(|card| card.width > 0.0 && card.height > 0.0)
        .map(|card| PhysicalRect {
            left: ox + (card.x * scale).round() as i32,
            top: oy + (card.y * scale).round() as i32,
            right: ox + ((card.x + card.width) * scale).round() as i32 + 1,
            bottom: oy + ((card.y + card.height) * scale).round() as i32 + 1,
        })
        .collect()
}

#[cfg(target_os = "windows")]
fn monitor_at(app: &AppHandle, px: f64, py: f64) -> Option<(Area, f64)> {
    let monitors = app.available_monitors().ok()?;
    let to_area = |m: &tauri::Monitor| {
        let scale = m.scale_factor();
        let work = m.work_area();
        let area = Area {
            left: work.position.x as f64 / scale,
            top: work.position.y as f64 / scale,
            right: (work.position.x as f64 + work.size.width as f64) / scale,
            bottom: (work.position.y as f64 + work.size.height as f64) / scale,
        };
        (area, scale)
    };
    monitors
        .iter()
        .find(|m| {
            let (pos, size) = (m.position(), m.size());
            px >= pos.x as f64
                && px < pos.x as f64 + size.width as f64
                && py >= pos.y as f64
                && py < pos.y as f64 + size.height as f64
        })
        .or_else(|| monitors.first())
        .map(to_area)
        .or_else(|| app.primary_monitor().ok().flatten().as_ref().map(to_area))
}

fn panel_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(PANEL_LABEL)
}

#[cfg(target_os = "windows")]
fn ensure_window(app: &AppHandle) -> Option<WebviewWindow> {
    if let Some(window) = panel_window(app) {
        return Some(window);
    }
    match create_window(app) {
        Ok(window) => Some(window),
        Err(err) => {
            tracing::error!(error = %err, "failed to create tray panel window");
            None
        }
    }
}

#[cfg(target_os = "windows")]
fn create_window(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    use tauri::{WebviewUrl, WebviewWindowBuilder};

    *lock_recover(&APPLIED_DARK) = None;
    let window =
        WebviewWindowBuilder::new(app, PANEL_LABEL, WebviewUrl::App("tray-panel.html".into()))
            .title("ice-box")
            .inner_size(MAIN_WIDTH, 340.0)
            .resizable(false)
            .maximizable(false)
            .minimizable(false)
            .decorations(false)
            .transparent(true)
            // Disable the outer rectangular DWM shadow: the fixed canvas spans
            // all three columns and a system shadow would reveal its bounds.
            .shadow(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .visible_on_all_workspaces(true)
            .visible(false)
            .focused(false)
            .build()?;

    let handle = window.clone();
    window.on_window_event(move |event| match event {
        tauri::WindowEvent::Focused(false) if handle.is_visible().unwrap_or(false) => {
            *lock_recover(&LAST_BLUR_HIDE) = Some(Instant::now());
            hide_window(&handle);
        }
        tauri::WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            hide_window(&handle);
        }
        _ => {}
    });
    Ok(window)
}

fn hide_window(window: &WebviewWindow) {
    // Advance `SEQ` and clear `ANCHOR` so any layout already queued onto the
    // main thread for this popup session becomes stale and is dropped.
    SEQ.fetch_add(1, Ordering::AcqRel);
    PENDING_REVEAL.store(false, Ordering::Release);
    *lock_recover(&ANCHOR) = None;
    *lock_recover(&APPLIED_FRAME) = None;
    let _ = window.hide();
    let _ = window.emit_to(PANEL_LABEL, HIDE_EVENT, ());
    #[cfg(target_os = "windows")]
    crate::tray_delay::cancel_and_drop(window.app_handle());
}

pub fn hide_panel(app: &AppHandle) {
    if let Some(window) = panel_window(app) {
        hide_window(&window);
    }
}

#[cfg(target_os = "windows")]
fn is_current_active_seq(seq: u64, window: &WebviewWindow) -> bool {
    SEQ.load(Ordering::Acquire) == seq
        && (PENDING_REVEAL.load(Ordering::Acquire) || window.is_visible().unwrap_or(false))
}

#[cfg(target_os = "windows")]
fn reveal(window: &WebviewWindow, seq: u64) {
    if SEQ.load(Ordering::Acquire) != seq || !PENDING_REVEAL.swap(false, Ordering::AcqRel) {
        return;
    }
    let _ = window.show();
    let _ = window.set_focus();
}

#[cfg(target_os = "windows")]
fn show_at(app: &AppHandle, window: &WebviewWindow, icon: (f64, f64, f64, f64)) {
    let (px, py, pw, ph) = icon;
    let Some((work, scale)) = monitor_at(app, px + pw / 2.0, py + ph / 2.0) else {
        tracing::warn!("tray panel: could not locate monitor for tray icon");
        return;
    };
    let seq = SEQ.fetch_add(1, Ordering::AcqRel) + 1;
    let (x, y, placement) = place(
        (px / scale, py / scale, pw / scale, ph / scale),
        work,
        seq,
        scale,
    );
    let (pre_x, pre_y) = preposition_origin(work, scale);
    platform::preposition_hidden(window, pre_x, pre_y);
    *lock_recover(&ANCHOR) = Some(Anchor {
        x,
        y,
        scale,
        placement: placement.clone(),
    });
    PENDING_REVEAL.store(true, Ordering::Release);
    *lock_recover(&APPLIED_FRAME) = None;
    let _ = window.emit_to(PANEL_LABEL, SHOW_EVENT, placement);
}

/// Toggle the custom tray panel beside the clicked tray icon rectangle.
#[cfg(target_os = "windows")]
pub fn toggle(app: &AppHandle, rect: &tauri::Rect) {
    if let Some(window) = panel_window(app) {
        if window.is_visible().unwrap_or(false) {
            hide_window(&window);
            return;
        }
    }
    let just_blurred = lock_recover(&LAST_BLUR_HIDE)
        .take()
        .is_some_and(|at| at.elapsed() < BLUR_CLICK_GRACE);
    if just_blurred {
        return;
    }
    let Some(window) = ensure_window(app) else {
        return;
    };
    let pos = rect.position.to_physical::<f64>(1.0);
    let size = rect.size.to_physical::<f64>(1.0);
    show_at(app, &window, (pos.x, pos.y, size.width, size.height));
}

fn is_panel_active(app: &AppHandle) -> bool {
    let Some(window) = panel_window(app) else {
        return false;
    };
    PENDING_REVEAL.load(Ordering::Acquire) || window.is_visible().unwrap_or(false)
}

/// Notify the open panel to refresh its model. No-op while the panel is hidden
/// (or on platforms without a custom panel), and throttled so bursts of delay
/// probe completions coalesce cleanly.
pub fn notify_changed(app: &AppHandle) {
    if !is_panel_active(app) {
        return;
    }
    let wait = {
        let mut state = lock_recover(&CHANGED_STATE);
        if state.1 {
            return;
        }
        let now = Instant::now();
        match state.0.map(|last| now.duration_since(last)) {
            Some(elapsed) if elapsed < CHANGED_THROTTLE => {
                state.1 = true;
                CHANGED_THROTTLE - elapsed
            }
            _ => {
                state.0 = Some(now);
                Duration::ZERO
            }
        }
    };
    if wait.is_zero() {
        let _ = app.emit_to(PANEL_LABEL, CHANGED_EVENT, ());
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        std::thread::sleep(wait);
        *lock_recover(&CHANGED_STATE) = (Some(Instant::now()), false);
        if is_panel_active(&app) {
            let _ = app.emit_to(PANEL_LABEL, CHANGED_EVENT, ());
        }
    });
}

#[cfg(target_os = "windows")]
mod platform {
    use super::{
        card_region_rects, compute_outer_rect, CardRect, Frame, PhysicalRect, CARD_RADIUS,
    };
    use tauri::WebviewWindow;
    use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        ClientToScreen, CombineRgn, CreateRoundRectRgn, DeleteObject, SetWindowRgn, HRGN, RGN_OR,
    };
    use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClientRect, GetWindowRect, SetWindowPos, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
    };

    /// Move the still-hidden window onto the target monitor so Windows updates
    /// its DPI (`WM_DPICHANGED`) before the frontend measures the popup layout.
    pub fn preposition_hidden(window: &WebviewWindow, x: i32, y: i32) {
        if window.is_visible().unwrap_or(true) {
            return;
        }
        let Ok(hwnd) = window.hwnd() else {
            return;
        };
        // SAFETY: `hwnd` is a live hidden window on the UI thread; moving it
        // with `SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE` neither shows nor
        // activates it.
        unsafe {
            SetWindowPos(
                hwnd.0 as HWND,
                std::ptr::null_mut(),
                x,
                y,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    fn to_phys(rect: RECT) -> PhysicalRect {
        PhysicalRect {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        }
    }

    fn set_client_frame(hwnd: HWND, frame: Frame, scale: f64) {
        let mut outer = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        let mut client = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        let mut origin = POINT { x: 0, y: 0 };
        // SAFETY: called on the UI thread for a live top-level window with
        // valid stack pointers for the output structs.
        unsafe {
            if GetWindowRect(hwnd, &mut outer) == 0
                || GetClientRect(hwnd, &mut client) == 0
                || ClientToScreen(hwnd, &mut origin) == 0
            {
                tracing::warn!("tray panel: failed to read Windows window frame metrics");
                return;
            }
            let rect = compute_outer_rect(
                frame,
                scale,
                to_phys(outer),
                to_phys(client),
                (origin.x, origin.y),
            );
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    /// Clip the fixed transparent canvas strictly to the union of all visible
    /// rounded menu cards so empty areas remain invisible and click-through.
    fn set_cards_region(hwnd: HWND, scale: f64, cards: &[CardRect]) -> bool {
        let mut outer = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        let mut origin = POINT { x: 0, y: 0 };
        // SAFETY: called on the UI thread for a live window; GDI region handles
        // created below are either freed on error/merge or transferred to the
        // OS by a successful `SetWindowRgn`.
        unsafe {
            if GetWindowRect(hwnd, &mut outer) == 0 || ClientToScreen(hwnd, &mut origin) == 0 {
                return false;
            }
            let rects =
                card_region_rects(cards, scale, (origin.x - outer.left, origin.y - outer.top));
            if rects.is_empty() {
                return false;
            }
            let diameter = (CARD_RADIUS * 2.0 * scale).round().max(2.0) as i32;
            let mut combined: HRGN = std::ptr::null_mut();
            for rect in rects {
                let next = CreateRoundRectRgn(
                    rect.left,
                    rect.top,
                    rect.right,
                    rect.bottom,
                    diameter,
                    diameter,
                );
                if next.is_null() {
                    if !combined.is_null() {
                        DeleteObject(combined);
                    }
                    tracing::warn!("tray panel: CreateRoundRectRgn failed");
                    return false;
                }
                if combined.is_null() {
                    combined = next;
                } else {
                    let result = CombineRgn(combined, combined, next, RGN_OR);
                    DeleteObject(next);
                    if result == 0 {
                        DeleteObject(combined);
                        tracing::warn!("tray panel: CombineRgn failed");
                        return false;
                    }
                }
            }
            if SetWindowRgn(hwnd, combined, 1) == 0 {
                DeleteObject(combined);
                tracing::warn!("tray panel: SetWindowRgn failed");
                return false;
            }
            true
        }
    }

    /// Position the window once per popup (re-calibrating across DPI changes)
    /// and update the clipped card region, revealing only once clipped.
    pub fn apply(
        window: &WebviewWindow,
        seq: u64,
        frame: Option<Frame>,
        scale: f64,
        cards: Vec<CardRect>,
    ) {
        let target = window.clone();
        let _ = window.run_on_main_thread(move || {
            // Skip layouts superseded by a hide or a newer popup session while
            // this closure waited in the main-thread queue.
            if !super::is_current_active_seq(seq, &target) {
                return;
            }
            let Ok(hwnd) = target.hwnd() else {
                return;
            };
            let raw = hwnd.0 as HWND;
            if let Some(frame) = frame {
                // SAFETY: `raw` is a live HWND owned by Tauri on this UI thread.
                unsafe {
                    let dpi = GetDpiForWindow(raw);
                    set_client_frame(raw, frame, scale);
                    // Moving onto a differently scaled monitor changes the
                    // non-client border widths via `WM_DPICHANGED`; align once more.
                    if GetDpiForWindow(raw) != dpi {
                        set_client_frame(raw, frame, scale);
                    }
                }
            }
            if set_cards_region(raw, scale, &cards) {
                super::reveal(&target, seq);
            }
        });
    }

    pub fn set_dark(window: &WebviewWindow, dark: Option<bool>) {
        let _ = window.set_theme(dark.map(|dark| {
            if dark {
                tauri::Theme::Dark
            } else {
                tauri::Theme::Light
            }
        }));
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use super::{CardRect, Frame};
    use tauri::WebviewWindow;

    pub fn apply(
        _window: &WebviewWindow,
        _seq: u64,
        _frame: Option<Frame>,
        _scale: f64,
        _cards: Vec<CardRect>,
    ) {
    }

    pub fn set_dark(_window: &WebviewWindow, _dark: Option<bool>) {}
}

/// Derive the current tray menu model off the main thread.
#[tauri::command]
pub async fn tray_panel_state(app: AppHandle) -> Result<crate::tray::PanelModel, String> {
    tauri::async_runtime::spawn_blocking(move || crate::tray::collect_panel_model(&app))
        .await
        .map_err(|err| err.to_string())
}

/// Read the active popup placement on initial panel mount (the first show
/// event may arrive before the fresh WebView registers its event listeners).
#[tauri::command]
pub fn tray_panel_placement(app: AppHandle) -> Option<PanelPlacement> {
    let visible = panel_window(&app).is_some_and(|window| window.is_visible().unwrap_or(false));
    if !PENDING_REVEAL.load(Ordering::Acquire) && !visible {
        return None;
    }
    lock_recover(&ANCHOR)
        .as_ref()
        .map(|anchor| anchor.placement.clone())
}

/// Apply the frontend's measured card layout: position the canvas once per
/// popup and update the clipped card region, revealing on the first layout.
#[tauri::command]
pub fn tray_panel_layout(app: AppHandle, layout: PanelLayout) {
    let Some(anchor) = lock_recover(&ANCHOR).clone() else {
        return;
    };
    if layout.seq != SEQ.load(Ordering::Acquire)
        || layout.seq != anchor.placement.seq
        || layout.width <= 0.0
        || layout.height <= 0.0
    {
        return;
    }
    let Some(window) = panel_window(&app) else {
        return;
    };
    {
        let mut applied = lock_recover(&APPLIED_DARK);
        if *applied != Some(layout.dark) {
            *applied = Some(layout.dark);
            platform::set_dark(&window, layout.dark);
        }
    }
    let frame = Frame {
        x: anchor.x - layout.offset_x,
        y: anchor.y + layout.offset_y,
        width: layout.width,
        height: layout.height,
    };
    let move_window = {
        let mut applied = lock_recover(&APPLIED_FRAME);
        let changed = *applied != Some(frame);
        *applied = Some(frame);
        changed
    };
    platform::apply(
        &window,
        layout.seq,
        move_window.then_some(frame),
        anchor.scale,
        layout.cards,
    );
}

#[tauri::command]
pub fn tray_panel_hide(app: AppHandle) {
    hide_panel(&app);
}

/// Dispatch a clicked panel row through the same action handler the native
/// menu uses. Must stay async so actions that focus or create windows cannot
/// deadlock WebView2 on Windows.
#[tauri::command]
pub async fn tray_panel_action(app: AppHandle, id: String) {
    let stays_open = crate::tray::panel_action_keeps_open(&id);
    if !stays_open {
        hide_panel(&app);
    }
    crate::tray::handle_panel_action(&app, &id);
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIN_WORK: Area = Area {
        left: 0.0,
        top: 0.0,
        right: 1920.0,
        bottom: 1032.0,
    };

    #[test]
    fn bottom_right_taskbar_opens_upward_and_cascades_left() {
        let (x, y, placement) = place((1760.0, 1040.0, 24.0, 40.0), WIN_WORK, 1, 1.0);
        assert_eq!(placement.edge, "bottom");
        assert_eq!(placement.side, "left");
        assert_eq!(y, WIN_WORK.bottom - SCREEN_MARGIN);
        assert!(x + placement.main_width <= WIN_WORK.right - SCREEN_MARGIN + 0.001);
        assert!(
            x - placement.gap - placement.sub_width - placement.gap - placement.leaf_width
                >= WIN_WORK.left + SCREEN_MARGIN - 0.001
        );
    }

    #[test]
    fn top_left_icon_opens_downward_and_cascades_right() {
        let (x, y, placement) = place((80.0, 0.0, 24.0, 32.0), WIN_WORK, 2, 1.0);
        assert_eq!(placement.edge, "top");
        assert_eq!(placement.side, "right");
        assert_eq!(y, 32.0 + ICON_GAP);
        assert!(x >= WIN_WORK.left + SCREEN_MARGIN - 0.001);
        assert!(
            x + placement.main_width
                + placement.gap
                + placement.sub_width
                + placement.gap
                + placement.leaf_width
                <= WIN_WORK.right - SCREEN_MARGIN + 0.001
        );
    }

    #[test]
    fn three_columns_fit_scaled_monitors_including_negative_coordinates() {
        for (width, height) in [(1280.0, 720.0), (1366.0, 768.0), (2560.0, 1440.0)] {
            for scale in [1.0, 1.25, 1.5, 2.0, 2.5] {
                for physical_left in [0.0, -width] {
                    let work = Area {
                        left: physical_left / scale,
                        top: 0.0,
                        right: (physical_left + width) / scale,
                        bottom: (height - 48.0) / scale,
                    };
                    for fraction in [0.08, 0.5, 0.94] {
                        let (x, y, p) = place(
                            (
                                work.left + (work.right - work.left) * fraction,
                                (height - 40.0) / scale,
                                24.0 / scale,
                                40.0 / scale,
                            ),
                            work,
                            1,
                            scale,
                        );
                        let (left, right) = if p.side == "left" {
                            (
                                x - p.gap - p.sub_width - p.gap - p.leaf_width,
                                x + p.main_width,
                            )
                        } else {
                            (
                                x,
                                x + p.main_width + p.gap + p.sub_width + p.gap + p.leaf_width,
                            )
                        };
                        assert!(
                            left >= work.left + SCREEN_MARGIN - 1.5,
                            "left overflow: {left} < {} ({width}x{height} @{scale}, frac={fraction})",
                            work.left + SCREEN_MARGIN
                        );
                        assert!(
                            right <= work.right - SCREEN_MARGIN + 1.5,
                            "right overflow: {right} > {} ({width}x{height} @{scale}, frac={fraction})",
                            work.right - SCREEN_MARGIN
                        );
                        assert!(y <= work.bottom - SCREEN_MARGIN + 0.001);
                    }
                }
            }
        }
    }

    #[test]
    fn outer_rect_compensates_non_client_borders_without_clipping() {
        let rect = compute_outer_rect(
            Frame {
                x: 1100.0,
                y: 620.0,
                width: 772.0,
                height: 400.0,
            },
            1.25,
            PhysicalRect {
                left: 1000,
                top: 500,
                right: 1772,
                bottom: 900,
            },
            PhysicalRect {
                left: 0,
                top: 0,
                right: 752,
                bottom: 389,
            },
            (1010, 501),
        );
        // 10px left/right border, 1px top border, 10px bottom border.
        assert_eq!(rect.left, 1375 - 10);
        assert_eq!(rect.top, 775 - 1);
        assert_eq!(rect.right - rect.left - 20, (772.0 * 1.25) as i32);
        assert_eq!(rect.bottom - rect.top - 11, (400.0 * 1.25) as i32);
    }

    #[test]
    fn card_region_rects_scale_and_preserve_each_card_independently() {
        let main = CardRect {
            x: 544.0,
            y: 440.0,
            width: 228.0,
            height: 320.0,
        };
        let mode = CardRect {
            x: 388.0,
            y: 480.0,
            width: 152.0,
            height: 92.0,
        };
        let empty = CardRect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 100.0,
        };
        let rects = card_region_rects(&[main, mode, empty], 1.5, (8, 1));
        assert_eq!(rects.len(), 2);
        assert_eq!(
            rects[0],
            PhysicalRect {
                left: 8 + 816,
                top: 1 + 660,
                right: 8 + 1158 + 1,
                bottom: 1 + 1140 + 1,
            }
        );
        assert_eq!(
            rects[1],
            PhysicalRect {
                left: 8 + 582,
                top: 1 + 720,
                right: 8 + 810 + 1,
                bottom: 1 + 858 + 1,
            }
        );
    }

    #[test]
    fn preposition_origin_maps_inside_target_monitor_including_negative_coords() {
        assert_eq!(preposition_origin(WIN_WORK, 1.5), (12, 12));
        let left_monitor = Area {
            left: -1280.0,
            top: 0.0,
            right: 0.0,
            bottom: 720.0,
        };
        assert_eq!(preposition_origin(left_monitor, 1.25), (-1590, 10));
    }
}
