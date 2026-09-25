// SPDX-License-Identifier: GPL-3.0-or-later

//! macOS tray menu: the delay test button as a custom view of its menu item.
//!
//! A menu item that gets picked closes the menu — that is how AppKit tracks a
//! menu, and no item can prevent it. For the delay test that is not an option:
//! the menu is where the progress and the results land, and closing it is the
//! cancel gesture (`tray_delay`), so the click that starts a run must leave the
//! menu open. An item with a `view` is the AppKit way out: the view is an
//! ordinary `NSView` that receives the click itself, and the menu never sees a
//! selection to close on.
//!
//! The view draws the row the menu would have drawn — the menu's own font, the
//! label colour (greyed out while a test runs and the row takes no click), and
//! a rounded highlight while the pointer is over it — because AppKit draws
//! nothing of its own for a view item. A click calls
//! [`crate::tray_delay::start`] with the scope the row stands for.
//!
//! One AppKit quirk is worked around (SO 15075033, Radar 7128269): the menu's
//! window is not the key window, and a view inside a menu item can stop
//! receiving mouse events once the menu has been shown a second time. The view
//! takes the window's key status back on its way into it and rebuilds its
//! tracking area there, the fix the report describes.

use crate::tray_delay::DelayScope;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{
    define_class, msg_send, AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly,
};
use objc2_app_kit::{
    NSAttributedStringNSStringDrawing, NSBezierPath, NSColor, NSEvent, NSFont, NSFontAttributeName,
    NSForegroundColorAttributeName, NSTrackingArea, NSTrackingAreaOptions, NSView, NSWindow,
};
use objc2_foundation::{NSAttributedString, NSDictionary, NSPoint, NSRect, NSSize, NSString};
use std::cell::Cell;
use tauri::AppHandle;

/// Row height of the view in points: the height macOS gives a menu row, so
/// putting a view on the item does not change the menu's shape.
const ROW_HEIGHT: f64 = 22.0;

/// Width the view starts at, before the menu places it: wide enough to hold
/// the label without widening the menu on its own (`fit_to_container` measures
/// the row it is actually put in).
const FALLBACK_WIDTH: f64 = 200.0;

/// Horizontal inset of the label in points: the indent macOS gives a menu
/// item's title, so the custom row's text lines up with the rows around it.
const LABEL_INSET: f64 = 14.0;

/// The hover highlight as AppKit draws it behind a highlighted row: a rounded
/// rectangle inset from the row's edges, in the system's grey selection colour
/// (dynamic, so it stays legible in the light and the dark appearance).
const HIGHLIGHT_INSET_X: f64 = 5.0;
const HIGHLIGHT_INSET_Y: f64 = 1.0;
const HIGHLIGHT_RADIUS: f64 = 4.0;

/// What one button row holds: the text it draws, whether it takes a click, and
/// the test a click starts.
struct DelayButtonIvars {
    /// Label as the menu model carried it at attach time.
    label: String,
    /// Whether the row takes a click (false while any test runs).
    enabled: bool,
    /// The page whose test a click starts.
    scope: DelayScope,
    /// The app to start the run with.
    app: AppHandle,
    /// Whether the pointer is over the row: drives the hover highlight.
    hovered: Cell<bool>,
    /// Whether the current press started inside the row: a press that leaves
    /// it again is not a click on it.
    pressed: Cell<bool>,
}

define_class!(
    /// The delay test button of one node page: a menu row that handles its own
    /// mouse events, so a click on it leaves the menu open.
    #[unsafe(super(NSView))]
    #[name = "IceBoxDelayButtonView"]
    #[thread_kind = MainThreadOnly]
    #[ivars = DelayButtonIvars]
    struct DelayButtonView;

    impl DelayButtonView {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty_rect: NSRect) {
            let ivars = self.ivars();
            let bounds = self.bounds();
            // A disabled row highlights like a disabled menu item: not at all.
            if ivars.enabled && (ivars.hovered.get() || ivars.pressed.get()) {
                let rect = NSRect::new(
                    NSPoint::new(HIGHLIGHT_INSET_X, HIGHLIGHT_INSET_Y),
                    NSSize::new(
                        (bounds.size.width - 2.0 * HIGHLIGHT_INSET_X).max(0.0),
                        (bounds.size.height - 2.0 * HIGHLIGHT_INSET_Y).max(0.0),
                    ),
                );
                NSColor::unemphasizedSelectedContentBackgroundColor().setFill();
                NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                    rect,
                    HIGHLIGHT_RADIUS,
                    HIGHLIGHT_RADIUS,
                )
                .fill();
            }
            let text = self.attributed_label();
            let size = text.size();
            // The label sits on the row's centre line like a menu item's
            // title. The view is not flipped, so y grows up from the bottom
            // edge and a rect as tall as the text centres it when placed by
            // its own height.
            text.drawInRect(NSRect::new(
                NSPoint::new(LABEL_INSET, (bounds.size.height - size.height) / 2.0),
                NSSize::new(
                    (bounds.size.width - LABEL_INSET - HIGHLIGHT_INSET_X).max(0.0),
                    size.height,
                ),
            ));
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            self.ivars().hovered.set(true);
            self.setNeedsDisplay(true);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            let ivars = self.ivars();
            ivars.hovered.set(false);
            ivars.pressed.set(false);
            self.setNeedsDisplay(true);
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            if self.ivars().enabled {
                self.ivars().pressed.set(true);
                self.setNeedsDisplay(true);
            }
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            let ivars = self.ivars();
            let started_here = ivars.pressed.replace(false);
            self.setNeedsDisplay(true);
            if started_here && ivars.enabled && self.contains(event) {
                crate::tray_delay::start(&ivars.app, ivars.scope.clone());
            }
        }

        /// See the module docs: without this, the row stops receiving mouse
        /// events once its menu has been shown a second time.
        #[unsafe(method(viewWillMoveToWindow:))]
        fn view_will_move_to_window(&self, new_window: Option<&NSWindow>) {
            // SAFETY: `super` is NSView's own implementation of the method, and
            // the message returns nothing.
            let () = unsafe { msg_send![super(self), viewWillMoveToWindow: new_window] };
            if let Some(window) = new_window {
                if !window.isKeyWindow() {
                    window.becomeKeyWindow();
                }
            }
            // Rebuild the tracked area through the method AppKit itself calls,
            // the way the fix is written in the report.
            let () = unsafe { msg_send![self, updateTrackingAreas] };
            self.fit_to_container();
        }

        #[unsafe(method(viewDidMoveToWindow))]
        fn view_did_move_to_window(&self) {
            // SAFETY: `super` is NSView's own implementation of the method, and
            // the message returns nothing.
            let () = unsafe { msg_send![super(self), viewDidMoveToWindow] };
            self.fit_to_container();
        }

        /// Rebuild the hover area inside the row, whatever the row measures
        /// now. AppKit calls this when the view's geometry changes, and the
        /// window fix above calls it for the same reason the report does.
        #[unsafe(method(updateTrackingAreas))]
        fn update_tracking_areas(&self) {
            // SAFETY: `super` is NSView's own implementation of the method, and
            // the message returns nothing.
            let () = unsafe { msg_send![super(self), updateTrackingAreas] };
            self.rebuild_tracking_area();
        }

        /// A row under a window that is not key still takes the first click.
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }
    }
);

impl DelayButtonView {
    /// Build the row for one page: its label, its clickability, and the test a
    /// click starts.
    fn new(
        mtm: MainThreadMarker,
        app: &AppHandle,
        label: &str,
        enabled: bool,
        scope: DelayScope,
    ) -> Retained<Self> {
        let ivars = DelayButtonIvars {
            label: label.to_string(),
            enabled,
            scope,
            app: app.clone(),
            hovered: Cell::new(false),
            pressed: Cell::new(false),
        };
        let this = mtm.alloc().set_ivars(ivars);
        // SAFETY: `initWithFrame:` is NSView's designated initializer, and the
        // ivars it may touch were set just above.
        unsafe {
            msg_send![
                super(this),
                initWithFrame: NSRect::new(
                    NSPoint::new(0.0, 0.0),
                    NSSize::new(FALLBACK_WIDTH, ROW_HEIGHT),
                )
            ]
        }
    }

    /// The label as the menu would draw it: the menu's font, in the label
    /// colour, or greyed out when the row takes no click.
    fn attributed_label(&self) -> Retained<NSAttributedString> {
        let ivars = self.ivars();
        // The menu font at its default size: `0.0` asks AppKit for the size
        // the menus themselves use.
        let font = NSFont::menuFontOfSize(0.0);
        let color = if ivars.enabled {
            NSColor::labelColor()
        } else {
            NSColor::disabledControlTextColor()
        };
        let values: [&AnyObject; 2] = [&font, &color];
        // SAFETY: AppKit's attribute-name constants are `&'static` keys, valid
        // for as long as the dictionary that copies them.
        let keys = unsafe { [NSFontAttributeName, NSForegroundColorAttributeName] };
        let attributes = NSDictionary::from_slices(&keys, &values);
        let text = NSString::from_str(&ivars.label);
        // SAFETY: each key is AppKit's attribute name, and each carries the
        // value AppKit documents it to take (an `NSFont`, an `NSColor`).
        unsafe { NSAttributedString::new_with_attributes(&text, &attributes) }
    }

    /// Whether `event` landed inside the row.
    fn contains(&self, event: &NSEvent) -> bool {
        let point = self.convertPoint_fromView(event.locationInWindow(), None);
        let bounds = self.bounds();
        point.x >= bounds.origin.x
            && point.x <= bounds.origin.x + bounds.size.width
            && point.y >= bounds.origin.y
            && point.y <= bounds.origin.y + bounds.size.height
    }

    /// Fill the item the view is put in: the container spans the menu row, and
    /// its width is only known once the menu has placed the view.
    fn fit_to_container(&self) {
        // SAFETY: reading the superview of a view that is being placed is
        // what the placement itself does.
        let Some(container) = (unsafe { self.superview() }) else {
            return;
        };
        let width = container.bounds().size.width;
        if width > 0.0 && (self.frame().size.width - width).abs() > 0.5 {
            self.setFrameSize(NSSize::new(width, ROW_HEIGHT));
        }
    }

    /// Replace the row's hover area with one spanning its current bounds.
    fn rebuild_tracking_area(&self) {
        for area in self.trackingAreas().iter() {
            self.removeTrackingArea(&area);
        }
        let options = NSTrackingAreaOptions::MouseEnteredAndExited
            | NSTrackingAreaOptions::ActiveAlways
            | NSTrackingAreaOptions::EnabledDuringMouseDrag
            | NSTrackingAreaOptions::InVisibleRect;
        // SAFETY: `InVisibleRect` makes the area follow the view's visible
        // rect, so the rect argument is ignored; the view owns the area and
        // receives the entered/exited events it is registered for.
        let area = unsafe {
            NSTrackingArea::initWithRect_options_owner_userInfo(
                NSTrackingArea::alloc(),
                self.bounds(),
                options,
                Some(self),
                None,
            )
        };
        self.addTrackingArea(&area);
    }
}

/// The clickable row of one page: a menu item view drawing `label`, taking a
/// click while `enabled`, and starting `scope`'s test with it. `None` when
/// there is no main thread to build the view on.
pub(crate) fn delay_button_view(
    app: &AppHandle,
    label: &str,
    enabled: bool,
    scope: DelayScope,
) -> Option<Retained<NSView>> {
    let mtm = MainThreadMarker::new()?;
    let view = DelayButtonView::new(mtm, app, label, enabled, scope);
    // The row follows the menu: the container it is put in is the full width
    // of the row, and stays that width when the menu is laid out again.
    view.setAutoresizingMask(objc2_app_kit::NSAutoresizingMaskOptions::ViewWidthSizable);
    Some(Retained::into_super(view))
}
