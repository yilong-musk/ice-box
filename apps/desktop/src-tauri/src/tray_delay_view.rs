// SPDX-License-Identifier: GPL-3.0-or-later

//! macOS tray menu: the delay test button as a custom view of its menu item.
//!
//! A menu item that gets picked closes the menu — that is how AppKit tracks a
//! menu, and no item can prevent it. For the delay test that is not an option:
//! the menu is where the progress and the results land, and closing it is the
//! cancel gesture (`tray_delay`), so the click that starts a run must leave the
//! menu open. An item with a `view` is the AppKit way out: the view is an
//! ordinary `NSView` that takes the click itself — and, like a control inside a
//! menu, it takes the whole press rather than its first event. `mouseDown:`
//! runs the tracking, pulling the drag and the release out of the queue before
//! the menu's own tracking can read them, so no completed click on the row is
//! left for the menu to close on.
//!
//! Drawing is the price of that view: AppKit leaves the row of an item with a
//! view to the view itself, highlight included. The row is therefore a
//! container with two subviews — the system's menu-selection material behind
//! (`highlight_view`), and the label the menu would have drawn on top
//! (`DelayLabelView`: same font, same colour, dimmed while the row takes no
//! click). Menus light their own rows up with that material, so the button
//! lights up the way the rows around it do rather than in a colour of its own.
//! A click calls [`crate::tray_delay::start`] with the scope the row stands
//! for.
//!
//! One AppKit quirk is worked around (SO 15075033, Radar 7128269): the menu's
//! window is not the key window, and a view inside a menu item can stop
//! receiving mouse events once the menu has been shown a second time. The view
//! takes the window's key status back on its way into it and rebuilds its
//! tracking area there, the fix the report describes.

use crate::tray_delay::DelayScope;
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool};
use objc2::{
    define_class, msg_send, AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly,
};
use objc2_app_kit::{
    NSAttributedStringNSStringDrawing, NSAutoresizingMaskOptions, NSBezierPath, NSColor, NSEvent,
    NSEventMask, NSEventTrackingRunLoopMode, NSEventType, NSFont, NSFontAttributeName,
    NSForegroundColorAttributeName, NSImage, NSTrackingArea, NSTrackingAreaOptions, NSView,
    NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView,
    NSWindow,
};
use objc2_foundation::{
    NSAttributedString, NSDate, NSDictionary, NSEdgeInsets, NSPoint, NSRect, NSSize, NSString,
};
use std::cell::{Cell, RefCell};
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

/// Where the highlight sits in the row, in points: a rounded rectangle inset
/// from the row's edges, the shape macOS gives the row under the pointer.
const HIGHLIGHT_INSET_X: f64 = 5.0;
const HIGHLIGHT_INSET_Y: f64 = 1.0;
const HIGHLIGHT_RADIUS: f64 = 4.0;

/// How long a tracked press waits for its release before it gives up: long
/// enough for any real click, short enough that a release that never arrives
/// cannot hold the main thread.
const TRACKING_TIMEOUT: f64 = 3.0;

thread_local! {
    /// The highlight's shape: one rounded-rectangle image serves every row, its
    /// cap insets stretching it to whatever size the menu hands the row.
    static HIGHLIGHT_MASK: RefCell<Option<Retained<NSImage>>> = const { RefCell::new(None) };
}

/// What one row's label holds: the text it draws, and whether the row takes a
/// click (the colour it draws that text in).
struct DelayLabelIvars {
    /// Label as the menu model carried it at the last refresh.
    label: RefCell<String>,
    /// Whether the row takes a click (false while any test runs).
    enabled: Cell<bool>,
}

define_class!(
    /// The text of one delay row: a drawing surface that sits above the row's
    /// highlight. It takes no mouse events of its own — the press belongs to
    /// the row around it, which is what keeps the menu open.
    #[unsafe(super(NSView))]
    #[name = "IceBoxDelayLabelView"]
    #[thread_kind = MainThreadOnly]
    #[ivars = DelayLabelIvars]
    struct DelayLabelView;

    impl DelayLabelView {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty_rect: NSRect) {
            let text = self.attributed_label();
            let size = text.size();
            let bounds = self.bounds();
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

        /// A press belongs to the row, not to the text drawn over it.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }
    }
);

impl DelayLabelView {
    /// Build the label of one row: the text it draws and whether the row takes
    /// a click. The row sets its frame.
    fn new(mtm: MainThreadMarker, label: &str, enabled: bool) -> Retained<Self> {
        let ivars = DelayLabelIvars {
            label: RefCell::new(label.to_string()),
            enabled: Cell::new(enabled),
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

    /// Draw `label` from now on, in the colour of a row that does or does not
    /// take a click.
    fn set_label(&self, label: &str, enabled: bool) {
        let ivars = self.ivars();
        ivars.label.replace(label.to_string());
        ivars.enabled.set(enabled);
        self.setNeedsDisplay(true);
    }

    /// The label as the menu would draw it: the menu's font, in the label
    /// colour, or greyed out when the row takes no click.
    fn attributed_label(&self) -> Retained<NSAttributedString> {
        let ivars = self.ivars();
        let label = ivars.label.borrow();
        // The menu font at its default size: `0.0` asks AppKit for the size
        // the menus themselves use.
        let font = NSFont::menuFontOfSize(0.0);
        let color = if ivars.enabled.get() {
            NSColor::labelColor()
        } else {
            NSColor::disabledControlTextColor()
        };
        let values: [&AnyObject; 2] = [&font, &color];
        // SAFETY: AppKit's attribute-name constants are `&'static` keys, valid
        // for as long as the dictionary that copies them.
        let keys = unsafe { [NSFontAttributeName, NSForegroundColorAttributeName] };
        let attributes = NSDictionary::from_slices(&keys, &values);
        let text = NSString::from_str(&label);
        // SAFETY: each key is AppKit's attribute name, and each carries the
        // value AppKit documents it to take (an `NSFont`, an `NSColor`).
        unsafe { NSAttributedString::new_with_attributes(&text, &attributes) }
    }
}

/// What one button row holds: whether it takes a click, the test a click
/// starts, where the pointer is, and the subviews that draw it.
struct DelayButtonIvars {
    /// Whether the row takes a click (false while any test runs).
    enabled: Cell<bool>,
    /// The page whose test a click starts.
    scope: DelayScope,
    /// The app to start the run with.
    app: AppHandle,
    /// Whether the pointer is over the row: while it is, the row is lit up.
    hovered: Cell<bool>,
    /// Whether the current press started inside the row: a press that leaves
    /// it again is not a click on it.
    pressed: Cell<bool>,
    /// The material behind the label, up only while the row is lit.
    highlight: RefCell<Option<Retained<NSVisualEffectView>>>,
    /// The label drawn over that material.
    label: RefCell<Option<Retained<DelayLabelView>>>,
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
        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            self.ivars().hovered.set(true);
            self.update_highlight();
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            let ivars = self.ivars();
            ivars.hovered.set(false);
            ivars.pressed.set(false);
            self.update_highlight();
        }

        /// A press on an enabled row tracks the mouse itself, the way a
        /// control inside a menu does, and that tracking is what keeps the menu
        /// open: the row takes the release out of the queue, so the menu never
        /// sees this click complete and has nothing to close on.
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            if !self.ivars().enabled.get() {
                return;
            }
            let ivars = self.ivars();
            ivars.pressed.set(true);
            ivars.hovered.set(true);
            self.update_highlight();
            self.track_press();
        }

        /// A press whose events never reached the tracking loop — there was no
        /// window to pull them from — still starts its run on the release.
        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            if !self.ivars().pressed.replace(false) {
                return;
            }
            self.end_press(self.contains(event));
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
            // The row was just put in place: take the hover state from the
            // pointer instead of waiting for an event that may not come.
            self.refresh_hover();
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
    /// Build the row for one page: the material that lights it up, the label
    /// over it, the clickability, and the test a click starts.
    fn new(
        mtm: MainThreadMarker,
        app: &AppHandle,
        label: &str,
        enabled: bool,
        scope: DelayScope,
    ) -> Retained<Self> {
        let ivars = DelayButtonIvars {
            enabled: Cell::new(enabled),
            scope,
            app: app.clone(),
            hovered: Cell::new(false),
            pressed: Cell::new(false),
            highlight: RefCell::new(None),
            label: RefCell::new(None),
        };
        let this = mtm.alloc().set_ivars(ivars);
        // SAFETY: `initWithFrame:` is NSView's designated initializer, and the
        // ivars it may touch were set just above.
        let view: Retained<Self> = unsafe {
            msg_send![
                super(this),
                initWithFrame: NSRect::new(
                    NSPoint::new(0.0, 0.0),
                    NSSize::new(FALLBACK_WIDTH, ROW_HEIGHT),
                )
            ]
        };
        // Subviews draw in the order they were added, so the highlight goes in
        // first and the label over it.
        let highlight = highlight_view(mtm, highlight_rect(view.bounds()));
        view.addSubview(&highlight);
        let label = DelayLabelView::new(mtm, label, enabled);
        label.setFrame(view.bounds());
        label.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        view.addSubview(&label);
        *view.ivars().highlight.borrow_mut() = Some(highlight);
        *view.ivars().label.borrow_mut() = Some(label);
        view
    }

    /// Follow a press to its release: pull the drag and the mouse-up events out
    /// of the queue, light the row up as the pointer moves in and out of it,
    /// and start the run when the release lands inside it.
    ///
    /// This is the tracking a control inside a menu does, and it is what keeps
    /// the menu open: the menu's own tracking never reads the release, so the
    /// click never becomes a selection it would close on. Only the events that
    /// belong to this press are taken; every other event stays in the queue.
    fn track_press(&self) {
        let Some(window) = self.window() else {
            self.end_press(false);
            return;
        };
        // A click's release is milliseconds away; the deadline is what keeps a
        // release that never arrives from holding the main thread.
        let deadline = NSDate::now().dateByAddingTimeInterval(TRACKING_TIMEOUT);
        let mask = NSEventMask::LeftMouseUp | NSEventMask::LeftMouseDragged;
        // SAFETY: `NSEventTrackingRunLoopMode` is AppKit's own run-loop mode
        // constant, the mode a press's tracking runs in; reading the static
        // only borrows the string it names.
        let mode = unsafe { NSEventTrackingRunLoopMode };
        while let Some(event) =
            window.nextEventMatchingMask_untilDate_inMode_dequeue(mask, Some(&deadline), mode, true)
        {
            let inside = self.contains(&event);
            let ivars = self.ivars();
            ivars.hovered.set(inside);
            self.update_highlight();
            if event.r#type() == NSEventType::LeftMouseUp {
                self.end_press(inside);
                return;
            }
        }
        // The deadline passed with the press still down: nothing to act on.
        self.end_press(false);
    }

    /// End a tracked press: drop the pressed state, keep the highlight in step
    /// with where the release landed, and start the run when that was inside an
    /// enabled row.
    fn end_press(&self, released_inside: bool) {
        let ivars = self.ivars();
        ivars.pressed.set(false);
        ivars.hovered.set(released_inside);
        self.update_highlight();
        if released_inside && ivars.enabled.get() {
            crate::tray_delay::start(&ivars.app, ivars.scope.clone());
        }
    }

    /// Draw `label` from now on, and take a click or not. A refresh rewrites the
    /// row through here rather than through a new view, so the item the open
    /// menu is tracking keeps the subviews it already laid out.
    fn set_label(&self, label: &str, enabled: bool) {
        self.ivars().enabled.set(enabled);
        if let Some(view) = self.ivars().label.borrow().as_ref() {
            view.set_label(label, enabled);
        }
        self.refresh_hover();
    }

    /// Light the row up while it takes a click and the pointer is over it — the
    /// way the menu lights up the rows around it — and put it out otherwise.
    fn update_highlight(&self) {
        let ivars = self.ivars();
        let lit = ivars.enabled.get() && ivars.hovered.get();
        if let Some(highlight) = ivars.highlight.borrow().as_ref() {
            highlight.setHidden(!lit);
        }
    }

    /// Take the hover state from where the pointer actually is. A menu tracks
    /// its own pointer, and a refresh carries no event of its own: a state
    /// inherited from the last refresh could leave a row lit with the pointer
    /// somewhere else.
    fn refresh_hover(&self) {
        let ivars = self.ivars();
        if ivars.pressed.get() {
            // The press tracking owns the state until it ends.
            return;
        }
        let hovered = self
            .window()
            .is_some_and(|window| self.contains_point(window.mouseLocationOutsideOfEventStream()));
        ivars.hovered.set(hovered);
        self.update_highlight();
    }

    /// Whether `event` landed inside the row.
    fn contains(&self, event: &NSEvent) -> bool {
        self.contains_point(event.locationInWindow())
    }

    /// Whether `point`, in the window's coordinates, is inside the row.
    fn contains_point(&self, point: NSPoint) -> bool {
        let point = self.convertPoint_fromView(point, None);
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

/// The highlight's place inside a row: the inner rectangle the menu itself
/// would light up.
fn highlight_rect(row: NSRect) -> NSRect {
    NSRect::new(
        NSPoint::new(HIGHLIGHT_INSET_X, HIGHLIGHT_INSET_Y),
        NSSize::new(
            (row.size.width - 2.0 * HIGHLIGHT_INSET_X).max(0.0),
            (row.size.height - 2.0 * HIGHLIGHT_INSET_Y).max(0.0),
        ),
    )
}

/// The highlight's shape as a stretchable mask: a rounded rectangle whose cap
/// insets hold the corners at [`HIGHLIGHT_RADIUS`] however wide the menu makes
/// the row. The material on its own would draw square corners.
fn highlight_mask() -> Retained<NSImage> {
    HIGHLIGHT_MASK.with(|slot| {
        let mut slot = slot.borrow_mut();
        if let Some(mask) = slot.as_ref() {
            return mask.clone();
        }
        let radius = HIGHLIGHT_RADIUS;
        let size = NSSize::new(2.0 * radius + 1.0, 2.0 * radius + 1.0);
        let handler = RcBlock::new(move |_rect: NSRect| -> Bool {
            NSColor::blackColor().setFill();
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                NSRect::new(NSPoint::new(0.0, 0.0), size),
                radius,
                radius,
            )
            .fill();
            Bool::YES
        });
        let mask = NSImage::imageWithSize_flipped_drawingHandler(size, false, &handler);
        mask.setCapInsets(NSEdgeInsets {
            top: radius,
            left: radius,
            bottom: radius,
            right: radius,
        });
        *slot = Some(mask.clone());
        mask
    })
}

/// The row's highlight: the system's menu-selection material, clipped to the
/// row's rounded inner rectangle. AppKit draws no highlight of its own for an
/// item whose view takes the click, and this is the material menus light their
/// rows up with — in the emphasized look, the accent tint a row takes while the
/// pointer is over it.
fn highlight_view(mtm: MainThreadMarker, frame: NSRect) -> Retained<NSVisualEffectView> {
    let view = NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(mtm), frame);
    view.setMaterial(NSVisualEffectMaterial::Selection);
    view.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
    view.setState(NSVisualEffectState::Active);
    view.setEmphasized(true);
    view.setMaskImage(Some(&highlight_mask()));
    // The insets the row starts with are the ones it keeps: the highlight
    // grows with the row rather than with its margins.
    view.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    view.setHidden(true);
    view
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
    view.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
    Some(Retained::into_super(view))
}

/// Retitle a row's view when it already carries one of ours: the label it
/// draws, and whether it takes a click. `false` when the view belongs to
/// someone else, which leaves the caller to put its own view on the row.
///
/// A refresh that only retitles takes this path rather than a fresh view, so
/// the row the open menu laid out keeps the subview in it.
pub(crate) fn retitle_button_view(view: Retained<NSView>, label: &str, enabled: bool) -> bool {
    let Ok(button) = view.downcast::<DelayButtonView>() else {
        return false;
    };
    button.set_label(label, enabled);
    true
}
