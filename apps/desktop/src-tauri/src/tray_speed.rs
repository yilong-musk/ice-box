// SPDX-License-Identifier: GPL-3.0-or-later

//! macOS menu bar: the live speed readout next to the tray icon.
//!
//! The readout is the newest Clash `/traffic` sample, re-derived once a second
//! and shown as two stacked lines (`↓ down` over `↑ up`) so the item stays
//! narrow. Two lines do not fit the menu bar in the default title font — 13pt
//! needs 32pt of height, more than an item gives its title, and macOS clips the
//! second line — so the text is drawn at [`READOUT_FONT_SIZE`] into an image of
//! our own, next to the tray icon: 9pt measures 22pt over both lines and stays
//! well under 50pt wide where the plain one-line title was ~153pt (both
//! measured on macOS 26).
//!
//! Composing the icon and the text into one image is what keeps the label on
//! the icon's centre line: a status item centres an image in its button, while
//! it pins an attributed title to the top of the item — AppKit drops the
//! leading, blank lines and paragraph spacing that would push a title down, and
//! a baseline offset only stretches the lines that follow it. The image is
//! drawn, not templated: the tray icon keeps its own colours, and the text
//! takes the menu bar's label colour, which AppKit resolves against the
//! appearance it draws the item in.
//!
//! A stopped proxy service dims the text to the same colour AppKit uses for
//! secondary labels (the service switch, not the core process: a core started
//! on its own runs with the service off), so the zeroes it prints while nothing
//! is proxied do not read as live traffic.

use crate::capture::TrafficCapture;
use crate::commands::{current_settings, proxy_service_posture};
use crate::tray::TRAY_ID;
use crate::AppState;
use block2::RcBlock;
use ice_config::TrayDisplayMode;
use ice_core::{CoreStatus, TimedTrafficSample};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool};
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSAttributedStringNSStringDrawing, NSColor, NSFont, NSFontAttributeName, NSFontWeightRegular,
    NSForegroundColorAttributeName, NSImage, NSLineBreakMode, NSMutableParagraphStyle,
    NSParagraphStyleAttributeName, NSStatusBarButton, NSTextAlignment,
};
use objc2_foundation::{NSAttributedString, NSDictionary, NSPoint, NSRect, NSSize, NSString};
use std::cell::RefCell;
use std::time::Duration;
use tauri::tray::TrayIcon;
use tauri::{AppHandle, Manager, Wry};

thread_local! {
    /// The icon Tauri installed in the status item. The readout replaces the
    /// button's image with icon and text composed together, so the plain icon
    /// is kept aside to compose from and to restore when the text clears.
    /// AppKit objects stay on the main thread, which is where the readout is
    /// applied.
    static PLAIN_ICON: RefCell<Option<Retained<NSImage>>> = const { RefCell::new(None) };
}

/// Readout cadence. The Clash `/traffic` stream ticks once per second, so
/// polling faster would only re-read the sample already on screen, and polling
/// slower would drop ticks between redraws.
const READOUT_INTERVAL: Duration = Duration::from_secs(1);

/// Age above which the newest sample counts as stale. The stream ticks once a
/// second and reconnects after a gap, so a reading this old means the monitor
/// is disconnected (core restarting, Clash API wedged) and the last rate must
/// not stay on screen as if it were live.
const READOUT_STALE_MS: u64 = 5_000;

/// Readout font size in points. Two lines at this size measure 22pt, which
/// clears the 18pt icon the tray uses and fills the height a status item gives
/// its content without reaching the menu bar's edges.
const READOUT_FONT_SIZE: f64 = 9.0;

/// The unit labels `format_rate` prints, in step order. [`readout_cell_width`]
/// measures all of them, so a unit that is not in this list could widen the
/// item; `format_rate` takes its labels from here.
const RATE_UNITS: [&str; 4] = ["B/s", "K/s", "M/s", "G/s"];

/// One figure space (U+2007), the width of one tabular digit — 5.84pt at 9pt in
/// the readout font, against 2.70pt for a plain space. Numbers are right-aligned
/// in their cell with these, so a short number keeps the unit beside it in
/// column without printing a leading zero to hold the place.
const FIGURE_SPACE: char = '\u{2007}';

/// Gap between the icon and the text column. Matches the spacing a status item
/// leaves between its image and its title.
const ICON_TEXT_GAP: f64 = 3.0;

/// Padding above and below the text inside the composed image, so the tallest
/// glyphs (`/`, the arrowheads) keep off the image bounds where a rounding step
/// could clip them. Symmetric, so it costs no alignment.
const READOUT_PADDING: f64 = 0.5;

/// Keep the menu-bar item in step with the `/traffic` stream the Home chart
/// reads, the `tray_display_mode` setting, and the core status the Home header
/// shows. Detaching the monitor (core stopped, teardown) drops its window,
/// which the readout prints as zero instead of leaving the last rate behind as
/// if the stream were still live, and the stopped service also dims the text
/// (see [`readout_dimmed`]).
pub fn spawn_watchdog(app: AppHandle) {
    std::thread::spawn(move || {
        // This loop is the only writer of the readout, so the cache needs no lock:
        // it keeps an unchanged item (idle traffic reads the same `0.0 B/s`
        // every tick, and past ticks reject the cache) from hopping to the main
        // thread every second.
        let mut applied: Option<(TrayDisplayMode, Option<String>, bool)> = None;
        loop {
            std::thread::sleep(READOUT_INTERVAL);
            let Some(state) = app.try_state::<AppState>() else {
                // Tauri drops managed state while the app tears down.
                break;
            };
            // Settings are read here rather than pushed by the Settings page:
            // the tray menu watchdog reads them the same way, and a failed read
            // keeps the previous item instead of guessing a mode.
            let Ok(settings) = current_settings(&state.paths) else {
                continue;
            };
            let mode = settings.tray_display_mode;
            // The readout dims with the *proxy service*, not with the core
            // process: the same answer Home's power control and the tray menu
            // switch give, from the same posture (a core started on its own
            // runs with the service off, and TUN keeps the service on through
            // its own, elevated core). The flag only changes what the readout
            // modes draw, but it is cheap to track for every mode: the item is
            // redrawn on a service switch either way.
            let running = state.core_snapshot.load().state.status == CoreStatus::Running;
            let tun_active = state.capture.status(&settings).traffic_capture == TrafficCapture::Tun;
            let service_on =
                proxy_service_posture(state.inner(), Some(&settings), running).engaged(tun_active);
            let readout = match mode {
                // No text to draw: skip the traffic read entirely.
                TrayDisplayMode::Icon => None,
                _ => Some(readout_text(
                    state.traffic.snapshot().points.last().copied(),
                    now_ms(),
                )),
            };
            let next = (mode, readout, service_on);
            if applied.as_ref() == Some(&next) {
                continue;
            }
            let Some(tray) = app.tray_by_id(TRAY_ID) else {
                continue;
            };
            // A failed apply keeps `applied` as it is, so the next tick retries.
            if set_readout(&tray, next.0, next.1.clone(), next.2) {
                applied = Some(next);
            }
        }
    });
}

/// Colour the readout draws in. Both are dynamic colours, resolved when the
/// status item draws the image, so the readout follows the menu bar's
/// appearance (dark mode, desktop tinting) even though it is baked into a
/// bitmap: the label colour while the proxy service is on, and the dimmed
/// secondary label colour — light grey against the bar's own background —
/// while it is off, so the zeroes an off service prints do not read as live
/// traffic.
fn readout_color(service_on: bool) -> Retained<NSColor> {
    if service_on {
        NSColor::labelColor()
    } else {
        NSColor::secondaryLabelColor()
    }
}

/// Readout for the newest Clash `/traffic` sample: `↓ down ↑ up` split over two
/// lines, in the units the Home chart's readout prints.
///
/// A sample that is missing (no stream attached — the monitor drops its window
/// when it is detached — or before the first tick) or older than
/// [`READOUT_STALE_MS`] reads as zero rather than clearing the text. The item
/// keeps its width while the proxy service starts and stops, and a core that is
/// not running really is carrying no traffic; clearing the text would shrink
/// the item and grow it again on every toggle. Zero is also what the wedged
/// stream case prints: the last rate must not stay on screen as if it were
/// live.
fn readout_text(latest: Option<TimedTrafficSample>, now_ms: u64) -> String {
    let live = latest.filter(|sample| now_ms.saturating_sub(sample.t) <= READOUT_STALE_MS);
    let (down, up) = live.map_or((0, 0), |sample| (sample.down, sample.up));
    format!("↓ {}\n↑ {}", format_rate(down), format_rate(up))
}

/// Rate text for the readout, 1024-based like the Home chart. A reading is one
/// decimal in three or four characters — `0.3 K/s`, `47.2 K/s` — right-aligned
/// in a fixed four-character cell by [`FIGURE_SPACE`] where a number is short,
/// so the item holds one width as the rate moves instead of gaining and losing
/// a character, and a number below ten reads `0.3` rather than `00.3`. The unit
/// labels ([`RATE_UNITS`]) are the short `B/s`, `K/s`, `M/s`, `G/s` the menu bar
/// has room for, where the Home chart spells out `KB/s` and `MB/s`.
///
/// The unit steps up before the number would need a fifth character, i.e. at
/// `99.95` of the current unit, where one decimal rounds to `100.0`. A rate
/// just over 100 B therefore reads `0.1 K/s` and one just over 100 KB reads
/// `0.1 M/s`. The Home chart (`formatRate` in
/// `apps/desktop/src/lib/traffic.ts`) keeps its own form, which prints whole
/// bytes and two decimals from a MiB up; the item is the one that has to stay
/// narrow.
fn format_rate(bytes_per_sec: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    const GIB: u64 = MIB * 1024;
    let bytes = bytes_per_sec as f64;
    let (unit, scaled) = if bytes < 99.95 {
        (0, bytes)
    } else if bytes < 99.95 * KIB as f64 {
        (1, bytes / KIB as f64)
    } else if bytes < 99.95 * MIB as f64 {
        (2, bytes / MIB as f64)
    } else {
        (3, bytes / GIB as f64)
    };
    let number = format!("{scaled:.1}");
    let pad = FIGURE_SPACE
        .to_string()
        .repeat(4usize.saturating_sub(number.chars().count()));
    format!("{pad}{number} {}", RATE_UNITS[unit])
}

/// Unix time in milliseconds, the clock the traffic monitor stamps samples
/// with. A clock before the epoch reads the newest sample as fresh, which only
/// means a stale rate could linger — the harmless side of the comparison.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since_epoch| since_epoch.as_millis() as u64)
        .unwrap_or(0)
}

/// Draw the item in `mode` with `readout` (the text to print, or `None` for the
/// icon alone), coloured by whether the proxy service is on. Returns whether the
/// status item was reached; the caller keeps its previous item on `false` (the
/// icon is not built yet, or the app is tearing down) and retries on the next
/// tick.
fn set_readout(
    tray: &TrayIcon<Wry>,
    mode: TrayDisplayMode,
    readout: Option<String>,
    service_on: bool,
) -> bool {
    tray.with_inner_tray_icon(move |tray| {
        let Some(mtm) = MainThreadMarker::new() else {
            return false;
        };
        let Some(status_item) = tray.ns_status_item() else {
            return false;
        };
        let Some(button) = status_item.button(mtm) else {
            return false;
        };
        let icon = plain_icon(&button);
        let (with_icon, text) = item_content(mode, readout);
        // The text lives in the image, so the title stays empty: it would be
        // drawn at the top of the item, and AppKit measures it into the item's
        // width.
        button.setAttributedTitle(&empty_title());
        match text {
            Some(readout) => {
                let icon = if with_icon { icon } else { None };
                let image = composed_image(icon, &readout, readout_color(service_on));
                button.setImage(Some(&image));
            }
            None => button.setImage(icon.as_deref()),
        }
        true
    })
    .unwrap_or(false)
}

/// What the item draws for `mode`: whether the icon appears, and the readout
/// text (`None` = no text).
///
/// The watchdog hands a readout to every mode that draws one, and only
/// [`TrayDisplayMode::Icon`] gets `None`, so the item never loses half of itself
/// when the proxy service starts or stops: a detached stream reads zero in
/// [`readout_text`] instead of clearing the text. [`TrayDisplayMode::Speed`]
/// still falls back to the icon when no text arrives, because an item with
/// neither icon nor text is an unclickable sliver of the menu bar, and the tray
/// menu is the window's only entry point while it is closed.
fn item_content(mode: TrayDisplayMode, readout: Option<String>) -> (bool, Option<String>) {
    match mode {
        TrayDisplayMode::Icon => (true, None),
        TrayDisplayMode::IconAndSpeed => (true, readout),
        TrayDisplayMode::Speed => match readout {
            Some(readout) => (false, Some(readout)),
            None => (true, None),
        },
    }
}

/// The icon the status item was built with, read off the button the first time
/// the readout replaces it. `None` when the tray has no icon, in which case the
/// readout stands alone.
fn plain_icon(button: &NSStatusBarButton) -> Option<Retained<NSImage>> {
    PLAIN_ICON.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            *slot = button.image();
        }
        slot.clone()
    })
}

/// An empty title: clears the text a status item would draw at the top of the
/// item without touching its image.
fn empty_title() -> Retained<NSAttributedString> {
    NSAttributedString::from_nsstring(&NSString::from_str(""))
}

/// What the status item draws: the tray icon with the two readout lines to its
/// right, both centred on the image's middle, so the item puts them on the menu
/// bar's centre line. The text always gets the same cell ([`readout_cell_width`]),
/// so the image — and with it the status item — keeps one width as the rate
/// moves.
fn composed_image(
    icon: Option<Retained<NSImage>>,
    readout: &str,
    color: Retained<NSColor>,
) -> Retained<NSImage> {
    let text = attributed_readout(readout, color);
    let text_size = text.size();
    let cell_width = readout_cell_width();
    let icon_size = icon
        .as_deref()
        .map(NSImage::size)
        .unwrap_or(NSSize::new(0.0, 0.0));
    let text_x = if icon.is_some() {
        icon_size.width + ICON_TEXT_GAP
    } else {
        0.0
    };
    let width = text_x + cell_width;
    let height = text_size.height.max(icon_size.height) + 2.0 * READOUT_PADDING;
    let handler = RcBlock::new(move |_rect: NSRect| -> Bool {
        // Inside the image the y axis grows up from the bottom edge, and both
        // the icon and the text block are placed by their own height, so the
        // two share one centre line however tall the other measures.
        if let Some(icon) = icon.as_deref() {
            let rect = NSRect::new(
                NSPoint::new(0.0, (height - icon_size.height) / 2.0),
                icon_size,
            );
            icon.drawInRect(rect);
        }
        // The block is centred inside the cell, which is at least as wide as
        // the widest reading, so a shorter reading sits in the middle of the
        // space the item reserves for it instead of shrinking the item.
        text.drawInRect(NSRect::new(
            NSPoint::new(text_x, (height - text_size.height) / 2.0),
            NSSize::new(cell_width, text_size.height),
        ));
        Bool::YES
    });
    NSImage::imageWithSize_flipped_drawingHandler(NSSize::new(width, height), false, &handler)
}

/// Width of the text cell every reading is drawn into.
///
/// The readout font's tabular digits (see [`readout_font`]) make every number
/// as wide as every other, but the unit letter is still proportional: `M` is
/// wider than `K`, and the composed image is only as wide as the text inside
/// it. Measuring all four units and taking the widest as the cell pins the
/// item's width, so stepping up a unit cannot shift the item — or everything
/// beside it in the menu bar — by a fraction of a point. Measured at 9pt on
/// macOS 26, `↓ 99.9 M/s` is the widest at 48.76pt, 1.94pt wider than the
/// `K/s` reading the item shows at ordinary rates.
///
/// Re-measured per compose rather than cached: a compose happens only when the
/// text changes, and four short strings cost far less than the drawing that
/// follows them. A rate past `99.95 G/s` — some 800 Gbps, more than any
/// interface the core can carry — would print a fifth character and outgrow
/// the cell; `format_rate` has no unit above `G/s`.
fn readout_cell_width() -> f64 {
    RATE_UNITS
        .iter()
        .map(|unit| {
            // The widest reading of every unit: two digits, one decimal, no
            // padding. A short number fills the same cell with figure spaces.
            attributed_readout(&format!("↓ 99.9 {unit}"), NSColor::labelColor())
                .size()
                .width
        })
        .fold(0.0_f64, f64::max)
}

/// The readout font: the menu bar's font at [`READOUT_FONT_SIZE`], with tabular
/// figures, so `11.1` is exactly as wide as `88.8` and the digits do not
/// shimmer or drag the item's width around as the rate moves. Measured on
/// macOS 26, the same reading set spans 41.86pt to 48.38pt in the proportional
/// system font.
fn readout_font() -> Retained<NSFont> {
    // SAFETY: AppKit declares the weight as a constant `CGFloat`; reading it
    // neither mutates nor races with anything.
    let weight = unsafe { NSFontWeightRegular };
    NSFont::monospacedDigitSystemFontOfSize_weight(READOUT_FONT_SIZE, weight)
}

/// The two readout lines as one block, centred, in the readout font and in
/// `color`. AppKit stacks the lines with the font's own leading — the compact
/// spacing the item wants — and the composed image decides where the block
/// lands.
fn attributed_readout(readout: &str, color: Retained<NSColor>) -> Retained<NSAttributedString> {
    let paragraph = NSMutableParagraphStyle::new();
    paragraph.setAlignment(NSTextAlignment::Center);
    paragraph.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
    let font = readout_font();
    let values: [&AnyObject; 3] = [&font, &paragraph, &color];
    // SAFETY: AppKit's attribute-name constants are `&'static` keys, valid for
    // as long as the dictionary that copies them.
    let keys = unsafe {
        [
            NSFontAttributeName,
            NSParagraphStyleAttributeName,
            NSForegroundColorAttributeName,
        ]
    };
    let attributes = NSDictionary::from_slices(&keys, &values);
    let text = NSString::from_str(readout);
    // SAFETY: each key is AppKit's attribute name, and each carries the value
    // AppKit documents it to take (an `NSFont`, an `NSParagraphStyle`, an
    // `NSColor`).
    unsafe { NSAttributedString::new_with_attributes(&text, &attributes) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tick(up: u64, down: u64, t: u64) -> TimedTrafficSample {
        TimedTrafficSample { up, down, t }
    }

    #[test]
    fn rates_step_up_a_unit_at_a_hundred() {
        // A number below ten keeps its cell with figure spaces, not zeroes.
        assert_eq!(format_rate(0), "\u{2007}0.0 B/s");
        assert_eq!(format_rate(3), "\u{2007}3.0 B/s");
        assert_eq!(format_rate(47), "47.0 B/s");
        assert_eq!(format_rate(99), "99.0 B/s");
        // Just over 100 B: a fraction of a KB, not three digits of bytes.
        assert_eq!(format_rate(100), "\u{2007}0.1 K/s");
        assert_eq!(format_rate(1023), "\u{2007}1.0 K/s");
        assert_eq!(format_rate(47 * 1024 + 204), "47.2 K/s");
        // The same step at 100 KB, and the same again at 100 MB.
        assert_eq!(format_rate(100 * 1024), "\u{2007}0.1 M/s");
        assert_eq!(format_rate(1024 * 1024), "\u{2007}1.0 M/s");
        assert_eq!(format_rate(3 * 1024 * 1024 / 2), "\u{2007}1.5 M/s");
        assert_eq!(format_rate(100 * 1024 * 1024), "\u{2007}0.1 G/s");
    }

    #[test]
    fn rates_step_up_before_rounding_to_a_fifth_character() {
        // 99.95 of a unit is where a tenth rounds to `100.0` — a digit more
        // than the readout keeps room for — so the step lands there.
        assert_eq!(format_rate(99), "99.0 B/s");
        assert_eq!(format_rate(100), "\u{2007}0.1 K/s");
        assert_eq!(format_rate(102_348), "99.9 K/s");
        assert_eq!(format_rate(102_349), "\u{2007}0.1 M/s");
        assert_eq!(format_rate(104_805_171), "99.9 M/s");
        assert_eq!(format_rate(104_805_172), "\u{2007}0.1 G/s");
    }

    #[test]
    fn rates_fill_one_number_cell_without_a_leading_zero() {
        let probes = [
            0,
            1,
            47,
            99,
            100,
            1_024,
            47 * 1_024,
            99 * 1_024,
            102_348,
            102_349,
            1024 * 1024,
            47 * 1024 * 1024,
            99 * 1024 * 1024,
            100 * 1024 * 1024,
            1024 * 1024 * 1024,
        ];
        for bytes in probes {
            let text = format_rate(bytes);
            let (cell, unit) = text
                .split_once(' ')
                .unwrap_or_else(|| panic!("{bytes}: {text} has no unit"));
            // One four-character cell whatever the reading: figure spaces stand
            // in for the digits a number below ten does not use.
            assert_eq!(cell.chars().count(), 4, "{bytes}: {cell:?}");
            assert!(
                cell.chars()
                    .all(|c| c == FIGURE_SPACE || c.is_ascii_digit() || c == '.'),
                "{bytes}: {cell:?}"
            );
            let number = cell.trim_start_matches(FIGURE_SPACE);
            let (whole, fraction) = number
                .split_once('.')
                .unwrap_or_else(|| panic!("{bytes}: {number} has no decimal"));
            // `0.5` and `47.2`, never `05.5` or `005.5`.
            assert!(
                (1..=2).contains(&whole.len()) && (whole.len() == 1 || !whole.starts_with('0')),
                "{bytes}: {number}"
            );
            assert_eq!(fraction.len(), 1, "{bytes}: {number}");
            assert!(RATE_UNITS.contains(&unit), "{bytes}: {unit}");
        }
    }

    #[test]
    fn rates_step_through_the_measured_units() {
        // `readout_cell_width` reserves room for exactly `RATE_UNITS`, and
        // `format_rate` prints exactly `RATE_UNITS`, in the same order: a unit
        // added to one list and not the other would let the item change width.
        let units: Vec<String> = [0, 100, 100 * 1024, 100 * 1024 * 1024]
            .into_iter()
            .map(|bytes| {
                format_rate(bytes)
                    .split(' ')
                    .nth(1)
                    .expect("rate text is `number unit`")
                    .to_string()
            })
            .collect();
        assert_eq!(units, RATE_UNITS);
    }

    #[test]
    fn readout_stacks_down_over_up() {
        assert_eq!(
            readout_text(Some(tick(1024, 2 * 1024 * 1024, 1_000)), 1_500),
            "↓ \u{2007}2.0 M/s\n↑ \u{2007}1.0 K/s"
        );
    }

    #[test]
    fn readout_has_no_gap_without_a_stream() {
        // A monitor without a stream or before its first tick (service stopped
        // or just started) reads zero: the item keeps the readout — and with it
        // its width — instead of dropping the text on every service toggle.
        assert_eq!(
            readout_text(None, 1_500),
            "↓ \u{2007}0.0 B/s\n↑ \u{2007}0.0 B/s".to_string()
        );
    }

    #[test]
    fn readout_zeroes_a_stale_sample() {
        let sample = tick(1024, 2 * 1024 * 1024, 1_000);
        // The last tick is still live at the threshold and reads zero one tick
        // later, instead of showing a rate the stream stopped updating.
        assert_eq!(
            readout_text(Some(sample), 1_000 + READOUT_STALE_MS),
            "↓ \u{2007}2.0 M/s\n↑ \u{2007}1.0 K/s"
        );
        assert_eq!(
            readout_text(Some(sample), 1_001 + READOUT_STALE_MS),
            "↓ \u{2007}0.0 B/s\n↑ \u{2007}0.0 B/s"
        );
    }

    #[test]
    fn display_modes_pick_the_parts_of_the_item() {
        let readout = "↓ \u{2007}2.0 M/s\n↑ \u{2007}1.0 K/s".to_string();
        // Icon + speed: both, whatever the stream does — a detached stream
        // reads zero (see `readout_has_no_gap_without_a_stream`) rather than
        // leaving the watchdog with no text to hand over.
        assert_eq!(
            item_content(TrayDisplayMode::IconAndSpeed, Some(readout.clone())),
            (true, Some(readout.clone()))
        );
        // Icon only: the readout is never drawn, so a detached stream and a
        // running one look the same.
        assert_eq!(
            item_content(TrayDisplayMode::Icon, Some(readout.clone())),
            (true, None)
        );
        // Speed only: the text without the icon, which is what keeps the item
        // clickable while no stream is attached; the icon is still the last
        // resort if a caller hands it no text at all.
        assert_eq!(
            item_content(TrayDisplayMode::Speed, Some(readout.clone())),
            (false, Some(readout))
        );
        assert_eq!(item_content(TrayDisplayMode::Speed, None), (true, None));
    }
}
