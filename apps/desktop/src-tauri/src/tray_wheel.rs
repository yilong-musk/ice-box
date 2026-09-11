// SPDX-License-Identifier: GPL-3.0-or-later

//! Windows-only: scroll the tray menu with the mouse wheel.
//!
//! The tray menu — its 「节点」and 「订阅」submenus included — is a classic Win32
//! popup menu (`TrackPopupMenu` over the menu muda builds), and those menus
//! ignore `WM_MOUSEWHEEL` entirely. A menu taller than the screen keeps its two
//! scroll arrows and the keyboard, so browsing a subscription with hundreds of
//! nodes means holding an arrow key. macOS and GTK menus scroll under the wheel
//! on their own; this module is the Windows counterpart.
//!
//! [`install`] hooks message processing on the thread that owns the tray
//! (`WH_GETMESSAGE` and `WH_MSGFILTER`, both thread-local, so no other
//! application's input is ever seen). While that thread runs a menu's modal
//! loop, a wheel message is translated into the arrow keys the menu already
//! understands: one item per line the OS reports for the wheel, the same unit
//! it uses for a list, with a partial notch waiting for the next message.
//! `SendInput` synthesizes the presses, so the menu's modal loop handles them
//! exactly like a real key press, which is the path the keyboard already takes.
//! The wheel message is consumed once translated, so a window behind the menu
//! (the app window, when the menu opened next to it) cannot scroll along.

use std::ffi::c_void;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, TRUE, WPARAM};
use windows_sys::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    VIRTUAL_KEY, VK_DOWN, VK_UP,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetClassNameW, GetGUIThreadInfo, GetWindowThreadProcessId, SetWindowsHookExW,
    SystemParametersInfoW, GUITHREADINFO, GUI_INMENUMODE, GUI_POPUPMENUMODE, GUI_SYSTEMMENUMODE,
    HC_ACTION, MSG, MSGF_MENU, PM_REMOVE, SPI_GETWHEELSCROLLLINES, WHEEL_DELTA, WH_GETMESSAGE,
    WH_MSGFILTER, WM_MOUSEWHEEL, WM_NULL,
};

/// Class name of every Win32 menu popup window.
const MENU_CLASS: [u16; 6] = [
    b'#' as u16,
    b'3' as u16,
    b'2' as u16,
    b'7' as u16,
    b'6' as u16,
    b'8' as u16,
];

/// Widest class name `GetClassNameW` reports; a name at the cap is truncated,
/// so it can never be a menu.
const MAX_CLASS_NAME: usize = 256;

/// `GetGUIThreadInfo` flags meaning "this thread is running a menu".
const MENU_MODE_FLAGS: u32 = GUI_INMENUMODE | GUI_POPUPMENUMODE | GUI_SYSTEMMENUMODE;

/// `SPI_GETWHEELSCROLLLINES` value for "one page per wheel notch".
const WHEEL_PAGESCROLL: u32 = u32::MAX;

/// One wheel notch in the signed arithmetic the delta needs.
const WHEEL_DELTA_I32: i32 = WHEEL_DELTA as i32;

/// Lines per notch when the OS reports a value a menu cannot use. Matches the
/// Windows default of three.
const DEFAULT_SCROLL_LINES: u32 = 3;

/// Cap on the lines honoured per notch: a menu has no page to scroll by, and a
/// user setting of a hundred lines would jump past everything on screen.
const MAX_SCROLL_LINES: u32 = 8;

/// Cap on the arrow presses one wheel message turns into, so a fast spin (a
/// driver can pack several notches into one message) stays one visible jump.
const MAX_STEPS: u32 = 32;

/// Set once both hooks are in place: installing twice would scroll two items
/// per notch.
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// Set by whichever hook translated a wheel message, so the other one does not
/// translate it a second time. The retrieval hook always runs before the menu
/// filter hook for one message, so a plain flag is enough.
static WHEEL_HANDLED: AtomicBool = AtomicBool::new(false);

/// Wheel delta left over from the last message. Precision wheels and touchpads
/// report a fraction of a notch at a time, so a fraction has to wait for the
/// ones after it instead of moving a whole notch worth of items.
static WHEEL_REMAINDER: AtomicI32 = AtomicI32::new(0);

/// Install the wheel hooks on the calling thread.
///
/// Must run on the thread that owns the tray (`setup_tray`, so the Tauri main
/// thread): both hooks only see that thread's messages, which include the ones
/// a menu's modal loop processes while the menu is open. The hooks stay for the
/// life of the thread — the menu can open at any time — and go away with the
/// process.
pub fn install() {
    if INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }
    // SAFETY: no arguments are involved; the id is always valid.
    let thread = unsafe { GetCurrentThreadId() };
    // Two hooks for one translation: `WH_GETMESSAGE` sees every message the
    // thread retrieves, `WH_MSGFILTER` the ones a menu's modal loop filters.
    // Which of the two a Windows release routes a menu message through is an
    // implementation detail, so both are installed and the first to see the
    // wheel translates it. `hMod` must be null for a hook on a thread of this
    // process, and both procedures are `'static` functions of this module.
    let retrieved = unsafe {
        SetWindowsHookExW(
            WH_GETMESSAGE,
            Some(get_message),
            std::ptr::null_mut(),
            thread,
        )
    };
    let filtered = unsafe {
        SetWindowsHookExW(
            WH_MSGFILTER,
            Some(filter_message),
            std::ptr::null_mut(),
            thread,
        )
    };
    if retrieved.is_null() {
        tracing::warn!(
            error = %std::io::Error::last_os_error(),
            "tray menu wheel retrieval hook install failed"
        );
    }
    if filtered.is_null() {
        tracing::warn!(
            error = %std::io::Error::last_os_error(),
            "tray menu wheel filter hook install failed"
        );
    }
    // Either half can carry the translation on its own; only a pair of failures
    // means there is no wheel scrolling to have.
    if retrieved.is_null() && filtered.is_null() {
        INSTALLED.store(false, Ordering::SeqCst);
    }
}

/// `WH_GETMESSAGE` callback: translate the wheel message the thread just
/// retrieved, when it belongs to one of our menus.
///
/// Runs on every message the hooked thread retrieves, so it stays down to two
/// comparisons until the message actually is a wheel message.
unsafe extern "system" fn get_message(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && wparam == PM_REMOVE as WPARAM {
        // SAFETY: `HC_ACTION` means `lparam` points at the message being
        // retrieved, which this thread owns until it dispatches it.
        let msg = lparam as *mut MSG;
        if unsafe { scroll_wheel_message(msg) } {
            WHEEL_HANDLED.store(true, Ordering::SeqCst);
            // The menu ignores the wheel itself; neutralizing the message keeps
            // it from reaching whatever window sits behind the menu.
            // SAFETY: `scroll_wheel_message` reported the pointer as a message.
            unsafe { (*msg).message = WM_NULL };
        }
    }
    // SAFETY: passing the call on is mandatory for message hooks, and a null
    // `hhk` is what a hook installed without a chain identity passes.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

/// `WH_MSGFILTER` callback: the same translation for the menu messages a modal
/// loop filters, in case this Windows routes the wheel there instead of through
/// [`get_message`].
unsafe extern "system" fn filter_message(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // For `MSGF_MENU` the message id arrives in `wparam` and the message itself
    // through `lparam`; reading the id first keeps the pointer dereference below
    // behind the platform's own contract.
    if code == MSGF_MENU as i32 && wparam == WM_MOUSEWHEEL as WPARAM {
        if WHEEL_HANDLED.swap(false, Ordering::SeqCst) {
            // The retrieval hook already translated this wheel message: this
            // second sighting must not scroll the menu again.
            return TRUE as LRESULT;
        }
        // SAFETY: `MSGF_MENU` hands the hook the message being processed.
        if unsafe { scroll_wheel_message(lparam as *const MSG) } {
            // Filter semantics: TRUE keeps the message from the menu, which
            // ignores the wheel anyway.
            return TRUE as LRESULT;
        }
    }
    // SAFETY: as above; not every filtered message is ours to drop.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

/// Translate one wheel message into menu scrolling, when it belongs to one of
/// our menus. `true` means the caller has to consume it.
///
/// # Safety
///
/// `msg` must be null, or point at the message a hook was handed.
unsafe fn scroll_wheel_message(msg: *const MSG) -> bool {
    if msg.is_null() {
        return false;
    }
    // SAFETY: the caller guarantees the pointer is a retrieved message.
    let msg = unsafe { &*msg };
    if msg.message != WM_MOUSEWHEEL || !menu_is_up(msg.hwnd) {
        return false;
    }
    // The wheel delta rides in the high word of `wParam`: positive for a roll
    // away from the user (scroll up), negative for a roll towards it.
    let notches = accumulate_notches((msg.wParam >> 16) as u16 as i16);
    if notches != 0 {
        let key = if notches > 0 { VK_UP } else { VK_DOWN };
        press_arrow(
            key,
            steps_for(
                notches.unsigned_abs(),
                items_per_notch(wheel_scroll_lines()),
            ),
        );
    }
    true
}

/// Whether the wheel belongs to a menu of ours: either this thread is running a
/// menu's modal loop, or the message went straight to one of our menu windows.
fn menu_is_up(wheel_window: HWND) -> bool {
    thread_in_menu_mode() || is_menu_window_of_this_process(wheel_window)
}

/// Whether the calling thread runs a menu's modal loop. `0` asks about the
/// calling thread, which is the one processing the message under the hook.
fn thread_in_menu_mode() -> bool {
    let mut info: GUITHREADINFO = unsafe { std::mem::zeroed() };
    info.cbSize = size_of::<GUITHREADINFO>() as u32;
    // SAFETY: `cbSize` is set, and the API fills the rest of the struct.
    let ok = unsafe { GetGUIThreadInfo(0, &mut info) };
    ok != 0 && (info.flags & MENU_MODE_FLAGS) != 0
}

/// Whether `hwnd` is a menu window (`#32768`) of this process.
fn is_menu_window_of_this_process(hwnd: HWND) -> bool {
    if hwnd.is_null() {
        return false;
    }
    let mut class = [0u16; MAX_CLASS_NAME];
    // SAFETY: the API writes at most `class.len()` wide characters.
    let len = unsafe { GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32) };
    if len <= 0 {
        return false;
    }
    let len = len as usize;
    if len != MENU_CLASS.len() || class[..len] != MENU_CLASS {
        return false;
    }
    let mut pid = 0u32;
    // SAFETY: the API writes the owning process id through the pointer.
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    // SAFETY: no arguments are involved; the id is always valid.
    pid == unsafe { GetCurrentProcessId() }
}

/// Menu items to move per wheel notch, from the OS wheel setting: the "lines"
/// count the mouse control panel edits, which is the unit lists scroll by.
fn wheel_scroll_lines() -> u32 {
    let mut setting = 0u32;
    // SAFETY: `SPI_GETWHEELSCROLLLINES` writes a `u32` through the pointer.
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETWHEELSCROLLLINES,
            0,
            std::ptr::addr_of_mut!(setting).cast::<c_void>(),
            0,
        )
    };
    if ok == 0 {
        return DEFAULT_SCROLL_LINES;
    }
    items_per_notch(setting)
}

/// Bound the raw `SPI_GETWHEELSCROLLLINES` value to what a menu can use: a menu
/// has no page size, and the wheel has to move at least one item to feel alive.
fn items_per_notch(setting: u32) -> u32 {
    if setting == WHEEL_PAGESCROLL {
        return MAX_SCROLL_LINES;
    }
    setting.clamp(1, MAX_SCROLL_LINES)
}

/// Notches this wheel message adds to the ones already waiting; the rest of a
/// partial notch stays in [`WHEEL_REMAINDER`] for the next message.
fn accumulate_notches(delta: i16) -> i32 {
    let pending = WHEEL_REMAINDER.swap(0, Ordering::SeqCst) + i32::from(delta);
    let (notches, remainder) = split_notches(pending);
    WHEEL_REMAINDER.store(remainder, Ordering::SeqCst);
    notches
}

/// Whole notches in `pending`, and the part of a notch that is not one yet.
fn split_notches(pending: i32) -> (i32, i32) {
    let notches = pending / WHEEL_DELTA_I32;
    (notches, pending - notches * WHEEL_DELTA_I32)
}

/// Arrow presses for `notches` wheel notches: the items per notch, bounded so
/// one message cannot jump past the whole menu.
fn steps_for(notches: u32, items_per_notch: u32) -> u32 {
    notches.saturating_mul(items_per_notch).min(MAX_STEPS)
}

/// Synthesize `steps` presses of `key`, as one `SendInput` batch so the menu
/// sees them in order without other input slipping in between.
fn press_arrow(key: VIRTUAL_KEY, steps: u32) {
    let mut inputs = Vec::with_capacity(steps as usize * 2);
    for _ in 0..steps {
        inputs.push(key_input(key, 0));
        inputs.push(key_input(key, KEYEVENTF_KEYUP));
    }
    // SAFETY: `inputs` is a valid `INPUT` array and `cbSize` is its element
    // size. A short count means the input was refused, which needs the menu to
    // belong to another process — impossible here — and leaves nothing to undo;
    // logging it per notch would only spam the menu path.
    let _ = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            size_of::<INPUT>() as i32,
        )
    };
}

fn key_input(key: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_notches_keeps_the_partial_one_for_later() {
        assert_eq!(split_notches(0), (0, 0));
        assert_eq!(split_notches(120), (1, 0));
        assert_eq!(split_notches(-240), (-2, 0));
        assert_eq!(split_notches(30), (0, 30));
        assert_eq!(split_notches(-250), (-2, -10));
    }

    #[test]
    fn notches_add_up_across_wheel_messages() {
        // A precision wheel reports a fraction of a notch per message: moving
        // has to wait until the fractions make a whole one.
        assert_eq!(accumulate_notches(30), 0);
        assert_eq!(accumulate_notches(30), 0);
        assert_eq!(accumulate_notches(30), 0);
        assert_eq!(accumulate_notches(30), 1);
        // A whole notch passes straight through, however many the driver packs
        // into one message.
        assert_eq!(accumulate_notches(-120), -1);
        assert_eq!(accumulate_notches(-240), -2);
        // Opposite directions cancel out, as they do in any other scroll area.
        assert_eq!(accumulate_notches(-60), 0);
        assert_eq!(accumulate_notches(60), 0);
    }

    #[test]
    fn steps_follow_the_line_setting() {
        // One notch is one "line" of the mouse settings, which a menu reads as
        // one item.
        assert_eq!(steps_for(1, 3), 3);
        assert_eq!(steps_for(2, 3), 6);
        assert_eq!(steps_for(1, 1), 1);
        // Bounded, so one message cannot jump past the whole menu.
        assert_eq!(steps_for(u32::MAX, MAX_SCROLL_LINES), MAX_STEPS);
    }

    #[test]
    fn items_per_notch_clamps_the_os_setting() {
        assert_eq!(items_per_notch(3), 3);
        assert_eq!(items_per_notch(0), 1);
        assert_eq!(items_per_notch(WHEEL_PAGESCROLL), MAX_SCROLL_LINES);
        assert_eq!(items_per_notch(1000), MAX_SCROLL_LINES);
    }
}
