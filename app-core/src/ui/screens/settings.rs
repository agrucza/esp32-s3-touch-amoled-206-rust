//! Settings screen - device configuration and diagnostics, organised
//! by hardware subsystem.
//!
//! Uses the **internal state machine** pattern: one [`SettingsScreen`]
//! struct holds a [`SettingsView`] enum that tracks which sub-view is
//! currently shown. Tapping a row in the Index sub-view switches
//! `view` to the corresponding sub-view; tapping the back chevron
//! returns to Index.
//!
//! Chrome follows the Nightwatch vocabulary: every sub-view shares a
//! [`header`] bar with chevron-left + title + right-aligned
//! telemetry + a 1-px signal hairline underline. The Index itself is
//! a stack of [`row`]s (icon / uppercase label / right control);
//! the leaf sub-views still use the rounded `card` + `value_body`
//! vocabulary inside, since those fit the tabular diagnostic data
//! better than a flat row list.

use embedded_graphics::{
    draw_target::{DrawTarget, DrawTargetExt},
    geometry::{Point, Size},
    pixelcolor::Rgb565,
    primitives::{Line, PrimitiveStyle, Rectangle, StyledDrawable},
};
use heapless::String;
use core::fmt::Write;

use crate::ui::{fonts, glyphs, layout, theme};
use crate::ui::types::{
    Action, Screen, SelfTestId, SelfTestResult, SystemData, SystemEvent,
};
use crate::ui::widgets::{
    card, chamfered_button, chamfered_panel, header, header_icon_hit, home_indicator, row,
    slider, slider_value_from_x, status_bar, tag_label, value_body, ButtonVariant, CardStyle,
    RowControl, Numpad, NumpadAction, NOTCH, ROW_H, SLIDER_BAR_H, STATUS_BAR_H, MAX_DIGITS,
};

/// Slider lower bound for brightness in the Display sub-view -
/// matches Quick Access. Anything dimmer is unreadable in practice
/// so the slider never goes below 5 %.
const BRIGHT_MIN_PCT: u8 = 5;

/// Auto-lock options surfaced in the Display sub-view, in the
/// order they appear (left -> right). `secs` is the off_timeout the
/// Model writes when this option is picked.
struct AutoLockOption {
    label: &'static str,
    secs: u32,
}

const AUTO_LOCK_OPTIONS: &[AutoLockOption] = &[
    AutoLockOption { label: "15S", secs: 15 },
    AutoLockOption { label: "30S", secs: 30 },
    AutoLockOption { label: "1M",  secs: 60 },
    AutoLockOption { label: "2M",  secs: 120 },
];

// -- Settings chrome helpers -------------------------------------------------

/// Y of the top status bar shared by every sub-view.
const STATUS_Y: i32 = 0;
/// Horizontal inset for status-bar content to clear the bezel arc.
const STATUS_X_INSET: i32 = 85;

/// Top of the Nightwatch header bar on settings sub-views. Sits
/// below the status bar with an 8 px gap so the two read as
/// separated.
const HDR_TOP: i32 = STATUS_Y + STATUS_BAR_H + 8;
/// Height of the Nightwatch header bar (see [`widgets::HEADER_H`]).
const HDR_H: i32 = 28;
/// Y of the bottom home-indicator bar.
const HOME_BAR_Y: i32 = theme::SCREEN_H as i32 - 18;

/// Header rect shared by every settings sub-view.
fn hdr_rect() -> Rectangle {
    Rectangle::new(
        Point::new(0, HDR_TOP),
        Size::new(theme::SCREEN_W as u32, HDR_H as u32),
    )
}

/// Draw the full Settings chrome: top status bar (tinted by `accent`,
/// carrying live HH:MM + battery% from `data`), Nightwatch header
/// with `title` + `SYS.CFG` telemetry, and bottom home-indicator bar.
fn draw_header<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    data: &SystemData,
    title: &str,
    accent: Rgb565,
) {
    let mut time_buf: heapless::String<8> = heapless::String::new();
    let _ = core::fmt::Write::write_fmt(
        &mut time_buf,
        format_args!("{:02}:{:02}", data.time.hour, data.time.minute),
    );
    status_bar(
        display,
        STATUS_Y,
        time_buf.as_str(),
        data.power.battery_percent,
        accent,
        STATUS_X_INSET,
    );

    header(display, hdr_rect(), title, "SYS.CFG", accent);

    home_indicator(display, HOME_BAR_Y, accent);
}

/// Y of the first row below the settings header.
const ROWS_TOP: i32 = HDR_TOP + HDR_H + 8;

/// Rect for the Nth row in the settings Index / Storage sub-index,
/// adjusted by the current scroll offset. `scroll = 0` returns the
/// row's natural position; positive `scroll` shifts everything up
/// (rows below come into view).
///
/// Rows span the full screen width; the `row` widget's internal
/// `ROW_PAD` keeps the icon, label, and hairline visually inset from
/// the bezel arc on its own. Letting the rect go edge-to-edge means
/// the row reads as a real list row rather than a floating panel.
fn row_rect(index: usize, scroll: i32) -> Rectangle {
    let y = ROWS_TOP + index as i32 * ROW_H - scroll;
    Rectangle::new(
        Point::new(0, y),
        Size::new(theme::SCREEN_W as u32, ROW_H as u32),
    )
}

/// Hit test the back chevron in the settings Nightwatch header.
fn header_back_hit(x: u16, y: u16) -> bool {
    header_icon_hit(x, y, hdr_rect())
}

// -- View enum ---------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsView {
    Index,
    Imu,
    Clock,
    TimeEntry,
    DateEntry,
    /// Battery status + history graph (samples from the flash event log).
    Battery,
    /// Storage sub-index. Routes to the storage leaves below.
    Storage,
    StorageFlash,
    StorageSd,
    StorageRestoreFlash,
    StorageFactoryReset,
    /// Display preferences (brightness slider + auto-lock). Stub
    /// for now; real contents land in W3d.
    Display,
    /// Wi-Fi configuration / status. Stub for now; real contents
    /// land when networking is wired up.
    Wifi,
    /// Bluetooth pairing / status. Stub for now; real contents
    /// land when BLE is wired up.
    Bluetooth,
    /// Zigbee mesh status. Only meaningful on the C6 board variant
    /// (S3 has no 802.15.4 radio); shown as a stub on S3 for now,
    /// to be feature-gated when the C6 build path lands.
    Zigbee,
}

// -- Index row metadata ------------------------------------------------------

/// Per-row icon. Rust can't coerce a generic `fn(..D..)` into a
/// non-generic function pointer, so we enum-dispatch to pick one of
/// a closed set of glyphs at render time (same pattern the App
/// Drawer uses).
#[derive(Clone, Copy)]
enum RowIcon {
    Clock,
    Battery,
    Imu,
    Storage,
    Flash,
    SdCard,
    Restore,
    Skull,
    NightMode,
    Sounds,
    Dnd,
    AlwaysOn,
    Display,
    Wifi,
    Bluetooth,
    Zigbee,
}

fn draw_row_icon<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, kind: RowIcon, cx: i32, cy: i32, color: Rgb565,
) {
    let r = 8;
    match kind {
        RowIcon::Clock     => glyphs::clock(display, cx, cy, r, color),
        RowIcon::Battery   => glyphs::battery(display, cx, cy, r, color),
        RowIcon::Imu       => glyphs::imu(display, cx, cy, r, color),
        RowIcon::Storage   => glyphs::chip(display, cx, cy, r, color),
        RowIcon::Flash     => glyphs::chip(display, cx, cy, r, color),
        RowIcon::SdCard    => glyphs::sd_card(display, cx, cy, r, color),
        RowIcon::Restore   => glyphs::chip(display, cx, cy, r, color),
        RowIcon::Skull     => glyphs::skull(display, cx, cy, r, color),
        RowIcon::NightMode => glyphs::moon(display, cx, cy, r, color),
        RowIcon::Sounds    => glyphs::bell(display, cx, cy, r, color),
        RowIcon::Dnd       => glyphs::dnd(display, cx, cy, r, color),
        RowIcon::AlwaysOn  => glyphs::power(display, cx, cy, r, color),
        RowIcon::Display   => glyphs::bolt(display, cx, cy, r, color),
        RowIcon::Wifi      => glyphs::signal_small(display, cx, cy, r, color),
        RowIcon::Bluetooth => glyphs::bluetooth_small(display, cx, cy, r, color),
        RowIcon::Zigbee    => glyphs::zigbee(display, cx, cy, r, color),
    }
}

/// What an index row does when tapped, plus how its right-control
/// renders. Navigate rows open a sub-view and show an inline status
/// value; toggle rows flip a config bool inline (no nav).
#[derive(Clone, Copy)]
enum RowKind {
    /// Tap opens `target`; the right side shows the inline value
    /// returned by `value_fn` (empty string => bare row, renders a
    /// chevron instead).
    Navigate {
        target: SettingsView,
        value_fn: fn(&SystemData) -> String<20>,
    },
    /// Tap fires `action` (typically a `Toggle*` config mutation).
    /// The right side shows a Nightwatch toggle reflecting `is_on`.
    Toggle {
        is_on: fn(&SystemData) -> bool,
        action: Action,
    },
}

struct IndexRow {
    label: &'static str,
    icon: RowIcon,
    kind: RowKind,
}

fn clock_value(data: &SystemData) -> String<20> {
    let mut buf = String::new();
    let _ = write!(buf, "{:02}:{:02}:{:02}", data.time.hour, data.time.minute, data.time.second);
    buf
}

fn imu_value(_data: &SystemData) -> String<20> {
    let mut buf = String::new();
    let _ = buf.push_str("QMI8658");
    buf
}

fn battery_value(data: &SystemData) -> String<20> {
    let mut buf = String::new();
    match data.power.battery_percent {
        Some(pct) => { let _ = write!(buf, "{}%", pct); }
        None      => { let _ = buf.push_str("--"); }
    }
    buf
}

fn storage_value(data: &SystemData) -> String<20> {
    // Summary shown on the top-level settings index: "<files> / <size>K".
    let mut buf = String::new();
    let _ = write!(
        buf,
        "{} / {}K",
        data.storage.files,
        data.storage.total_bytes / 1024,
    );
    buf
}

// -- Storage sub-index rows --------------------------------------------------
//
// Same IndexRow pattern as the top-level settings index, one level
// deeper. Each row taps into a storage leaf view.

fn storage_flash_value(data: &SystemData) -> String<20> {
    let mut buf = String::new();
    let _ = write!(
        buf,
        "{} FILES / {}K",
        data.storage.files,
        data.storage.total_bytes / 1024,
    );
    buf
}

fn storage_sd_value(data: &SystemData) -> String<20> {
    let mut buf = String::new();
    let _ = buf.push_str(if data.storage.sd_online { "ONLINE" } else { "NOT PRESENT" });
    buf
}

fn storage_reset_value(_data: &SystemData) -> String<20> {
    String::new()
}

fn storage_restore_value(data: &SystemData) -> String<20> {
    let mut buf = String::new();
    let _ = buf.push_str(if data.storage.sd_online { "" } else { "SD NOT PRESENT" });
    buf
}

fn night_mode_is_on(data: &SystemData) -> bool {
    data.config.display.night_mode
}

fn always_on_is_on(data: &SystemData) -> bool {
    data.config.display.always_on
}

fn haptics_is_on(data: &SystemData) -> bool {
    data.config.haptics_enabled
}

fn dnd_is_on(data: &SystemData) -> bool {
    data.config.dnd
}

const STORAGE_INDEX_ROWS: &[IndexRow] = &[
    IndexRow {
        label: "FLASH",
        icon: RowIcon::Flash,
        kind: RowKind::Navigate { target: SettingsView::StorageFlash, value_fn: storage_flash_value },
    },
    IndexRow {
        label: "SD CARD",
        icon: RowIcon::SdCard,
        kind: RowKind::Navigate { target: SettingsView::StorageSd, value_fn: storage_sd_value },
    },
    IndexRow {
        label: "RESTORE FROM SD",
        icon: RowIcon::Restore,
        kind: RowKind::Navigate { target: SettingsView::StorageRestoreFlash, value_fn: storage_restore_value },
    },
    IndexRow {
        label: "FACTORY RESET",
        icon: RowIcon::Skull,
        kind: RowKind::Navigate { target: SettingsView::StorageFactoryReset, value_fn: storage_reset_value },
    },
];

/// Empty inline value - causes navigate rows to render a chevron
/// instead of an inline string.
fn empty_value(_data: &SystemData) -> String<20> { String::new() }

const INDEX_ROWS: &[IndexRow] = &[
    // Spec prefs first - most-used live up top.
    IndexRow {
        label: "DISPLAY",
        icon: RowIcon::Display,
        kind: RowKind::Navigate { target: SettingsView::Display, value_fn: empty_value },
    },
    IndexRow {
        label: "SOUNDS",
        icon: RowIcon::Sounds,
        kind: RowKind::Toggle { is_on: haptics_is_on, action: Action::ToggleHaptics },
    },
    IndexRow {
        label: "DND",
        icon: RowIcon::Dnd,
        kind: RowKind::Toggle { is_on: dnd_is_on, action: Action::ToggleDnd },
    },
    IndexRow {
        label: "WIFI",
        icon: RowIcon::Wifi,
        kind: RowKind::Navigate { target: SettingsView::Wifi, value_fn: empty_value },
    },
    IndexRow {
        label: "BLUETOOTH",
        icon: RowIcon::Bluetooth,
        kind: RowKind::Navigate { target: SettingsView::Bluetooth, value_fn: empty_value },
    },
    IndexRow {
        label: "ZIGBEE",
        icon: RowIcon::Zigbee,
        kind: RowKind::Navigate { target: SettingsView::Zigbee, value_fn: empty_value },
    },
    // Diagnostic / drill rows.
    IndexRow {
        label: "CLOCK",
        icon: RowIcon::Clock,
        kind: RowKind::Navigate { target: SettingsView::Clock, value_fn: clock_value },
    },
    IndexRow {
        label: "BATTERY",
        icon: RowIcon::Battery,
        kind: RowKind::Navigate { target: SettingsView::Battery, value_fn: battery_value },
    },
    IndexRow {
        label: "6-AXIS IMU",
        icon: RowIcon::Imu,
        kind: RowKind::Navigate { target: SettingsView::Imu, value_fn: imu_value },
    },
    IndexRow {
        label: "STORAGE",
        icon: RowIcon::Storage,
        kind: RowKind::Navigate { target: SettingsView::Storage, value_fn: storage_value },
    },
    // Destructive action - last, danger-tinted icon. Re-uses the
    // existing Factory Reset sub-view (the spec's Purge+Reset and
    // our Factory Reset are the same destructive action).
    IndexRow {
        label: "PURGE+RESET",
        icon: RowIcon::Skull,
        kind: RowKind::Navigate { target: SettingsView::StorageFactoryReset, value_fn: empty_value },
    },
];

// -- IMU sub-view test list --------------------------------------------------

struct ImuTestRow {
    label: &'static str,
    id: SelfTestId,
    unit: &'static str,
}

const IMU_TESTS: &[ImuTestRow] = &[
    ImuTestRow {
        label: "ACCEL SELF-TEST",
        id: SelfTestId::ImuAccel,
        unit: "mg",
    },
    ImuTestRow {
        label: "GYRO SELF-TEST",
        id: SelfTestId::ImuGyro,
        unit: "dps",
    },
];

// -- Numpad time label Y (same as timer) -------------------------------------

const NUMPAD_TIME_Y: i32 = 90;

// -- SettingsScreen ----------------------------------------------------------

pub struct SettingsScreen {
    view: SettingsView,
    numpad: Numpad,
    /// Vertical scroll state for the index sub-view. Other sub-views
    /// don't currently scroll; if they ever do, give them their own
    /// `ScrollState` instance.
    index_scroll: layout::ScrollState,
}

impl SettingsScreen {
    pub fn new() -> Self {
        Self {
            view: SettingsView::Index,
            numpad: Numpad::new(6),
            index_scroll: layout::ScrollState::new(),
        }
    }
}

// -- Screen impl -------------------------------------------------------------

impl Screen for SettingsScreen {
    fn render<D: DrawTarget<Color = Rgb565>>(&self, display: &mut D, data: &SystemData) {
        match self.view {
            SettingsView::Index => self.render_index(display, data),
            SettingsView::Imu => self.render_imu(display, data),
            SettingsView::Clock => self.render_clock(display, data),
            SettingsView::TimeEntry => self.render_time_entry(display, data),
            SettingsView::DateEntry => self.render_date_entry(display, data),
            SettingsView::Battery => self.render_battery(display, data),
            SettingsView::Storage => self.render_storage_index(display, data),
            SettingsView::StorageFlash => self.render_storage_flash(display, data),
            SettingsView::StorageSd => self.render_storage_sd(display, data),
            SettingsView::StorageRestoreFlash => self.render_storage_restore(display, data),
            SettingsView::StorageFactoryReset => self.render_storage_factory_reset(display, data),
            SettingsView::Display   => self.render_display(display, data),
            SettingsView::Wifi      => self.render_stub(display, data, "WIFI"),
            SettingsView::Bluetooth => self.render_stub(display, data, "BLUETOOTH"),
            SettingsView::Zigbee    => self.render_stub(display, data, "ZIGBEE"),
        }
    }

    fn on_event(&mut self, event: &SystemEvent, data: &mut SystemData) -> Action {
        if matches!(event, SystemEvent::PowerButtonLong) {
            return Action::Shutdown;
        }

        match self.view {
            SettingsView::Index => self.index_event(event, data),
            SettingsView::Imu => self.imu_event(event, data),
            SettingsView::Clock => self.clock_event(event, data),
            SettingsView::TimeEntry => self.time_entry_event(event, data),
            SettingsView::DateEntry => self.date_entry_event(event, data),
            SettingsView::Battery => self.battery_event(event, data),
            SettingsView::Storage => self.storage_index_event(event, data),
            SettingsView::StorageFlash => self.storage_flash_event(event, data),
            SettingsView::StorageSd => self.storage_sd_event(event, data),
            SettingsView::StorageRestoreFlash => self.storage_restore_event(event, data),
            SettingsView::StorageFactoryReset => self.storage_factory_reset_event(event, data),
            SettingsView::Display => self.display_event(event, data),
            SettingsView::Wifi
            | SettingsView::Bluetooth
            | SettingsView::Zigbee => self.stub_event(event, data),
        }
    }
}

// -- Index sub-view ----------------------------------------------------------

impl SettingsScreen {
    fn render_index<D: DrawTarget<Color = Rgb565>>(
        &self, display: &mut D, data: &SystemData,
    ) {
        // Chrome (status bar, header, home indicator) renders against
        // the un-clipped target so it stays fixed regardless of scroll.
        draw_header(display, data, "SETTINGS", theme::SIGNAL);

        // Rows render into a clipped sub-target spanning the area
        // between the header bottom and the home indicator. Anything
        // that scrolls past those bounds is hard-clipped at the
        // hardware level.
        let viewport = index_viewport_rect();
        let scroll = self.index_scroll.offset();
        let mut clipped = display.clipped(&viewport);
        render_rows(&mut clipped, data, INDEX_ROWS, scroll);
    }

    fn index_event(&mut self, event: &SystemEvent, _data: &mut SystemData) -> Action {
        match event {
            SystemEvent::Tap { x, y } if header_back_hit(*x, *y) => {
                Action::Back
            }
            SystemEvent::Tap { x, y } => {
                if let Some(action) = row_hit(
                    *x, *y, INDEX_ROWS,
                    self.index_scroll.offset(),
                    &index_viewport_rect(),
                    &mut self.view,
                ) {
                    return action;
                }
                Action::None
            }
            // Finger drag on the index scrolls the row list. First
            // TouchPressed of a gesture records the y; subsequent
            // ones translate finger motion into scroll deltas.
            SystemEvent::TouchPressed { y, .. } => {
                let max = index_scroll_max();
                if self.index_scroll.drag(*y as i32, max) {
                    return Action::Redraw;
                }
                Action::None
            }
            SystemEvent::TouchReleased => {
                self.index_scroll.release();
                Action::None
            }
            _ => Action::None,
        }
    }
}

/// Visible-row viewport rect for the index. Spans from just below
/// the header hairline to just above the home-indicator bar.
fn index_viewport_rect() -> Rectangle {
    let top = ROWS_TOP;
    let bot = HOME_BAR_Y - 4;
    Rectangle::new(
        Point::new(0, top),
        Size::new(theme::SCREEN_W as u32, (bot - top) as u32),
    )
}

/// Maximum valid scroll offset for the index based on the current
/// row count and viewport height. Zero when content fits.
fn index_scroll_max() -> i32 {
    let viewport_h = index_viewport_rect().size.height as i32;
    let content_h = INDEX_ROWS.len() as i32 * ROW_H;
    (content_h - viewport_h).max(0)
}

// -- Shared row rendering / hit-testing for index + storage sub-index --------

/// Render a stack of [`IndexRow`]s using `nightwatch::row`. Navigate
/// rows show an inline value (or a chevron when the value is empty);
/// toggle rows show a Nightwatch toggle. `scroll` shifts each row's
/// y by `-scroll` so the caller can render into a clipped viewport.
fn render_rows<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, data: &SystemData, rows: &[IndexRow], scroll: i32,
) {
    for (i, r) in rows.iter().enumerate() {
        let rect = row_rect(i, scroll);
        let kind = r.icon;
        match r.kind {
            RowKind::Navigate { value_fn, .. } => {
                let val = value_fn(data);
                let control = if val.is_empty() {
                    RowControl::Chevron(theme::CYAN)
                } else {
                    RowControl::Inline(val.as_str(), theme::FG_MUTED)
                };
                row(
                    display, rect,
                    |d, cx, cy, c| draw_row_icon(d, kind, cx, cy, c),
                    theme::CYAN,
                    r.label,
                    control,
                );
            }
            RowKind::Toggle { is_on, .. } => {
                row(
                    display, rect,
                    |d, cx, cy, c| draw_row_icon(d, kind, cx, cy, c),
                    theme::CYAN,
                    r.label,
                    RowControl::Toggle(is_on(data)),
                );
            }
        }
    }
}

/// Row hit test, scroll-aware. Returns the `Action` the tap should
/// produce, or `None` if the tap missed every row. Taps outside
/// `viewport` are rejected (so a tap landing on the chrome area
/// doesn't accidentally trigger a row that happens to be scrolled
/// into the chrome's pixels). Navigate rows update the caller's
/// `view` via the `&mut SettingsView` and return `Action::Redraw`;
/// toggle rows return their own action variant.
fn row_hit(
    x: u16, y: u16, rows: &[IndexRow],
    scroll: i32, viewport: &Rectangle,
    view: &mut SettingsView,
) -> Option<Action> {
    let pt = Point::new(x as i32, y as i32);
    if !viewport.contains(pt) { return None; }
    for (i, r) in rows.iter().enumerate() {
        if !row_rect(i, scroll).contains(pt) { continue; }
        return Some(match r.kind {
            RowKind::Navigate { target, .. } => {
                *view = target;
                Action::Redraw
            }
            RowKind::Toggle { action, .. } => action,
        });
    }
    None
}

// -- Clock sub-view (time + date cards) --------------------------------------

impl SettingsScreen {
    fn render_clock<D: DrawTarget<Color = Rgb565>>(
        &self, display: &mut D, data: &SystemData,
    ) {
        draw_header(display, data, "CLOCK", theme::SIGNAL);

        // Time card.
        let rect = layout::content_card_rect(0);
        card(display, rect, CardStyle::DEFAULT);
        let mut time_buf: String<12> = String::new();
        let _ = write!(time_buf, "{:02}:{:02}:{:02}",
            data.time.hour, data.time.minute, data.time.second);
        value_body(display, rect, "TIME", time_buf.as_str(), theme::FG);

        // Date card.
        let rect = layout::content_card_rect(1);
        card(display, rect, CardStyle::DEFAULT);
        let mut date_buf: String<12> = String::new();
        let _ = write!(date_buf, "{:02}.{:02}.{:04}",
            data.time.day, data.time.month, data.time.year);
        value_body(display, rect, "DATE", date_buf.as_str(), theme::FG);
    }

    fn clock_event(&mut self, event: &SystemEvent, data: &mut SystemData) -> Action {
        match event {
            // Keep the display fresh.
            SystemEvent::TimeUpdated { .. } => Action::Redraw,

            SystemEvent::Tap { x, y } if header_back_hit(*x, *y) => {
                self.view = SettingsView::Index;
                Action::Redraw
            }
            SystemEvent::Swipe {
                dir: crate::events::SwipeDir::Right,
                region: crate::events::SwipeRegion::Content,
            } => {
                self.view = SettingsView::Index;
                Action::Redraw
            }
            SystemEvent::Tap { x, y } => {
                if layout::content_card_rect(0)
                    .contains(Point::new(*x as i32, *y as i32))
                {
                    // Open time numpad, pre-fill with current time.
                    let t = &data.time;
                    self.numpad = Numpad::new(6);
                    self.numpad.prefill(&[
                        t.hour / 10, t.hour % 10,
                        t.minute / 10, t.minute % 10,
                        t.second / 10, t.second % 10,
                    ]);
                    self.view = SettingsView::TimeEntry;
                    Action::Redraw
                } else if layout::content_card_rect(1)
                    .contains(Point::new(*x as i32, *y as i32))
                {
                    // Open date numpad, pre-fill with current date.
                    let t = &data.time;
                    self.numpad = Numpad::new(8);
                    self.numpad.prefill(&[
                        t.day / 10, t.day % 10,
                        t.month / 10, t.month % 10,
                        (t.year / 1000) as u8, ((t.year / 100) % 10) as u8,
                        ((t.year / 10) % 10) as u8, (t.year % 10) as u8,
                    ]);
                    self.view = SettingsView::DateEntry;
                    Action::Redraw
                } else {
                    Action::None
                }
            }
            _ => Action::None,
        }
    }
}

// -- Time entry numpad -------------------------------------------------------

impl SettingsScreen {
    fn render_time_entry<D: DrawTarget<Color = Rgb565>>(
        &self, display: &mut D, data: &SystemData,
    ) {
        draw_header(display, data, "SET TIME", theme::SIGNAL);

        // HH:MM:SS label from digits.
        let p = pad_digits(&self.numpad.digits, 6);
        let mut buf: String<12> = String::new();
        let _ = write!(buf, "{}{}{}{}{}{}{}{}",
            p[0], p[1], ':', p[2], p[3], ':', p[4], p[5]);
        fonts::draw_centered(
            display, &fonts::value(),
            &buf,
            theme::SCREEN_W as i32 / 2, NUMPAD_TIME_Y,
            theme::SIGNAL,
        );

        self.numpad.render(display);
    }

    fn time_entry_event(&mut self, event: &SystemEvent, data: &mut SystemData) -> Action {
        match event {
            SystemEvent::Tap { x, y } if header_back_hit(*x, *y) => {
                self.numpad = Numpad::new(6);
                self.view = SettingsView::Clock;
                Action::Redraw
            }
            SystemEvent::Tap { x, y } => {
                if let Some(action) = self.numpad.hit_test(*x, *y) {
                    match action {
                        NumpadAction::Confirm => {
                            let p = pad_digits(&self.numpad.digits, 6);
                            let h = p[0] * 10 + p[1];
                            let m = p[2] * 10 + p[3];
                            let s = p[4] * 10 + p[5];
                            // Validate.
                            if h < 24 && m < 60 && s < 60 {
                                self.numpad = Numpad::new(6);
                                self.view = SettingsView::Clock;
                                return Action::SetTime {
                                    year: data.time.year,
                                    month: data.time.month,
                                    day: data.time.day,
                                    hour: h,
                                    minute: m,
                                    second: s,
                                };
                            }
                            // Invalid - just redraw (user can see the bad value).
                            Action::Redraw
                        }
                        other => {
                            if self.numpad.apply(other) {
                                Action::Redraw
                            } else {
                                Action::None
                            }
                        }
                    }
                } else {
                    Action::None
                }
            }
            _ => Action::None,
        }
    }
}

// -- Date entry numpad -------------------------------------------------------

impl SettingsScreen {
    fn render_date_entry<D: DrawTarget<Color = Rgb565>>(
        &self, display: &mut D, data: &SystemData,
    ) {
        draw_header(display, data, "SET DATE", theme::SIGNAL);

        // DD.MM.YYYY label from digits.
        let p = pad_digits(&self.numpad.digits, 8);
        let mut buf: String<12> = String::new();
        let _ = write!(buf, "{}{}.{}{}.{}{}{}{}",
            p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7]);
        fonts::draw_centered(
            display, &fonts::value(),
            &buf,
            theme::SCREEN_W as i32 / 2, NUMPAD_TIME_Y,
            theme::SIGNAL,
        );

        self.numpad.render(display);
    }

    fn date_entry_event(&mut self, event: &SystemEvent, data: &mut SystemData) -> Action {
        match event {
            SystemEvent::Tap { x, y } if header_back_hit(*x, *y) => {
                self.numpad = Numpad::new(6);
                self.view = SettingsView::Clock;
                Action::Redraw
            }
            SystemEvent::Tap { x, y } => {
                if let Some(action) = self.numpad.hit_test(*x, *y) {
                    match action {
                        NumpadAction::Confirm => {
                            let p = pad_digits(&self.numpad.digits, 8);
                            let d = p[0] * 10 + p[1];
                            let m = p[2] * 10 + p[3];
                            let y = p[4] as u16 * 1000 + p[5] as u16 * 100
                                  + p[6] as u16 * 10 + p[7] as u16;
                            // Basic validation.
                            if d >= 1 && d <= 31 && m >= 1 && m <= 12 && y >= 2000 && y <= 2099 {
                                self.numpad = Numpad::new(6);
                                self.view = SettingsView::Clock;
                                return Action::SetTime {
                                    year: y,
                                    month: m,
                                    day: d,
                                    hour: data.time.hour,
                                    minute: data.time.minute,
                                    second: data.time.second,
                                };
                            }
                            Action::Redraw
                        }
                        other => {
                            if self.numpad.apply(other) {
                                Action::Redraw
                            } else {
                                Action::None
                            }
                        }
                    }
                } else {
                    Action::None
                }
            }
            _ => Action::None,
        }
    }
}

// -- IMU sub-view ------------------------------------------------------------

impl SettingsScreen {
    fn render_imu<D: DrawTarget<Color = Rgb565>>(
        &self,
        display: &mut D,
        data: &SystemData,
    ) {
        draw_header(display, data, "6-AXIS IMU", theme::SIGNAL);

        // Per test: a state-display panel (tag-labeled, border + text
        // tinted by run state) plus a separate primary button below
        // that triggers the test. Splits "show state" from "do
        // thing" so the panel is read-only and there's an explicit
        // tap target.
        let slots = imu_slots();
        for (i, test) in IMU_TESTS.iter().enumerate() {
            let (panel_rect, button_rect) = slots[i];
            let result = data.self_tests[test.id as usize];
            let (value_buf, _, _) = format_result(&result, test.unit);
            let accent = imu_result_accent(&result);

            chamfered_panel(display, panel_rect, NOTCH, accent, 1);
            tag_label(
                display,
                panel_rect.top_left.x,
                panel_rect.top_left.y,
                test.label,
                accent,
                NOTCH,
            );
            fonts::draw_centered_in_rect(
                display, &fonts::value(),
                value_buf.as_str(), panel_rect, accent,
            );

            // Button: Primary while idle/finished, Ghost while a test
            // is running so the user can't re-tap mid-run.
            let running = matches!(result, SelfTestResult::Running);
            let variant = if running {
                ButtonVariant::Ghost
            } else {
                ButtonVariant::Primary
            };
            chamfered_button(
                display, button_rect, "RUN SELF-TEST",
                variant, theme::SIGNAL,
            );
        }
    }

    fn imu_event(&mut self, event: &SystemEvent, data: &mut SystemData) -> Action {
        match event {
            SystemEvent::Tap { x, y } if header_back_hit(*x, *y) => {
                self.view = SettingsView::Index;
                Action::Redraw
            }
            SystemEvent::Swipe {
                dir: crate::events::SwipeDir::Right,
                region: crate::events::SwipeRegion::Content,
            } => {
                self.view = SettingsView::Index;
                Action::Redraw
            }
            SystemEvent::Tap { x, y } => {
                let pt = Point::new(*x as i32, *y as i32);
                let slots = imu_slots();
                for (i, test) in IMU_TESTS.iter().enumerate() {
                    let (_, button_rect) = slots[i];
                    if !button_rect.contains(pt) { continue; }
                    // The button is rendered in Ghost variant when this
                    // test is running - mirror that visual by ignoring
                    // the tap so behavior matches what the user sees.
                    if matches!(data.self_tests[test.id as usize], SelfTestResult::Running) {
                        return Action::None;
                    }
                    return Action::RunSelfTest(test.id);
                }
                Action::None
            }
            SystemEvent::SelfTestUpdated { .. } => Action::Redraw,
            _ => Action::None,
        }
    }

    // -- Battery sub-view ----------------------------------------------------

    fn render_battery<D: DrawTarget<Color = Rgb565>>(
        &self,
        display: &mut D,
        data: &SystemData,
    ) {
        draw_header(display, data, "BATTERY", theme::SIGNAL);

        // Top: chamfered tag-labeled BATTERY panel with live
        // percent/voltage centered inside.
        let panel_w = theme::SCREEN_W as i32 - 56;
        let panel_x = (theme::SCREEN_W as i32 - panel_w) / 2;
        let panel_y = ROWS_TOP + 18;
        let panel_h = 60i32;
        let panel_rect = Rectangle::new(
            Point::new(panel_x, panel_y),
            Size::new(panel_w as u32, panel_h as u32),
        );
        chamfered_panel(display, panel_rect, NOTCH, theme::SIGNAL, 1);
        tag_label(
            display,
            panel_rect.top_left.x,
            panel_rect.top_left.y,
            "NOW",
            theme::SIGNAL,
            NOTCH,
        );
        let mut val: String<20> = String::new();
        match (data.power.battery_percent, data.power.battery_voltage_mv) {
            (Some(pct), Some(mv)) => {
                let _ = write!(val, "{}% / {}.{:02}V", pct, mv / 1000, (mv % 1000) / 10);
            }
            (Some(pct), None) => { let _ = write!(val, "{}%", pct); }
            _                  => { let _ = val.push_str("--"); }
        }
        fonts::draw_centered_in_rect(
            display, &fonts::value(),
            val.as_str(), panel_rect, theme::FG,
        );

        // Sparkline: full screen width, edge-to-edge, no card around.
        let graph_y = panel_y + panel_h + 14;
        let graph_h = 96i32;
        let graph_rect = Rectangle::new(
            Point::new(0, graph_y),
            Size::new(theme::SCREEN_W as u32, graph_h as u32),
        );
        draw_battery_sparkline(display, graph_rect, &data.battery_history);

        // Below the sparkline: UPTIME stat in a smaller chamfered
        // tag-labeled panel.
        let uptime_y = graph_y + graph_h + 14;
        let uptime_rect = Rectangle::new(
            Point::new(panel_x, uptime_y),
            Size::new(panel_w as u32, panel_h as u32),
        );
        chamfered_panel(display, uptime_rect, NOTCH, theme::CYAN, 1);
        tag_label(
            display,
            uptime_rect.top_left.x,
            uptime_rect.top_left.y,
            "UPTIME",
            theme::CYAN,
            NOTCH,
        );
        let mut up_buf: String<16> = String::new();
        let total = data.uptime_secs;
        let h = total / 3600;
        let m = (total % 3600) / 60;
        let s = total % 60;
        let _ = write!(up_buf, "{:02}:{:02}:{:02}", h, m, s);
        fonts::draw_centered_in_rect(
            display, &fonts::value(),
            up_buf.as_str(), uptime_rect, theme::FG,
        );
    }

    fn battery_event(&mut self, event: &SystemEvent, _data: &mut SystemData) -> Action {
        match event {
            SystemEvent::Tap { x, y } if header_back_hit(*x, *y) => {
                self.view = SettingsView::Index;
                Action::Redraw
            }
            SystemEvent::Swipe {
                dir: crate::events::SwipeDir::Right,
                region: crate::events::SwipeRegion::Content,
            } => {
                self.view = SettingsView::Index;
                Action::Redraw
            }
            // Any live snapshot refresh or new sample should repaint.
            SystemEvent::PowerUpdated { .. }
            | SystemEvent::BatteryChanged { .. }
            | SystemEvent::TimeUpdated { .. } => Action::Redraw,
            _ => Action::None,
        }
    }
}

// -- Storage sub-views -------------------------------------------------------
//
// Two-level hierarchy:
//
//   Settings → Storage (sub-index) → { Flash | SD Card | Factory Reset }
//
// The sub-index mirrors the top-level settings index layout (one
// row per leaf). Each leaf is a focused view for its single
// concern. Back navigation from a leaf returns to the Storage
// sub-index; back from the sub-index returns to the Settings index.

impl SettingsScreen {
    // -- Storage sub-index ---------------------------------------------------

    fn render_storage_index<D: DrawTarget<Color = Rgb565>>(
        &self,
        display: &mut D,
        data: &SystemData,
    ) {
        draw_header(display, data, "STORAGE", theme::SIGNAL);
        // Storage sub-index doesn't scroll today (4 rows always
        // fit). Scroll = 0; if more storage rows land later,
        // give SettingsScreen a second `ScrollState` and viewport.
        render_rows(display, data, STORAGE_INDEX_ROWS, 0);
    }

    fn storage_index_event(&mut self, event: &SystemEvent, _data: &mut SystemData) -> Action {
        match event {
            SystemEvent::Tap { x, y } if header_back_hit(*x, *y) => {
                self.view = SettingsView::Index;
                Action::Redraw
            }
            SystemEvent::Swipe {
                dir: crate::events::SwipeDir::Right,
                region: crate::events::SwipeRegion::Content,
            } => {
                self.view = SettingsView::Index;
                Action::Redraw
            }
            SystemEvent::Tap { x, y } => {
                if let Some(action) = row_hit(
                    *x, *y, STORAGE_INDEX_ROWS,
                    0, &index_viewport_rect(),
                    &mut self.view,
                ) {
                    return action;
                }
                Action::None
            }
            SystemEvent::StorageUsageUpdated { .. } => Action::Redraw,
            _ => Action::None,
        }
    }

    // -- Flash leaf (read-only info) -----------------------------------------

    fn render_storage_flash<D: DrawTarget<Color = Rgb565>>(
        &self,
        display: &mut D,
        data: &SystemData,
    ) {
        draw_header(display, data, "FLASH", theme::SIGNAL);

        // Chamfered HUD panel with a hanging FLASH tag ribbon - the
        // spec's "tag-labelled panel" idiom. Body carries the usage
        // numbers as mono-ish body text.
        let margin = 28i32;
        let panel_w = theme::SCREEN_W as i32 - margin * 2;
        let panel_h = 120i32;
        let panel_rect = Rectangle::new(
            Point::new(margin, ROWS_TOP + 24),
            Size::new(panel_w as u32, panel_h as u32),
        );
        // Symmetric chamfered panel (Nightwatch default - TL + BR both cut).
        chamfered_panel(display, panel_rect, NOTCH, theme::SIGNAL, 1);

        // Tag ribbon sits exactly at the panel's TL corner. Its own
        // TL chamfer of size NOTCH carves out the same triangular
        // area as the panel's TL chamfer so the two align pixel-
        // for-pixel.
        tag_label(
            display,
            panel_rect.top_left.x,
            panel_rect.top_left.y,
            "FLASH",
            theme::SIGNAL,
            NOTCH,
        );

        // Interior: usage line centered vertically in the full panel
        // rect. The tag sits in the top-left corner and doesn't
        // interfere with a single centered line of body text.
        let mut buf: String<32> = String::new();
        let _ = write!(
            buf,
            "{} FILES / {} KB",
            data.storage.files,
            data.storage.total_bytes / 1024,
        );
        fonts::draw_centered_in_rect(
            display, &fonts::value(),
            buf.as_str(), panel_rect, theme::FG,
        );
    }

    fn storage_flash_event(&mut self, event: &SystemEvent, _data: &mut SystemData) -> Action {
        match event {
            SystemEvent::Tap { x, y } if header_back_hit(*x, *y) => {
                self.view = SettingsView::Storage;
                Action::Redraw
            }
            SystemEvent::Swipe {
                dir: crate::events::SwipeDir::Right,
                region: crate::events::SwipeRegion::Content,
            } => {
                self.view = SettingsView::Storage;
                Action::Redraw
            }
            SystemEvent::StorageUsageUpdated { .. } => Action::Redraw,
            _ => Action::None,
        }
    }

    // -- SD card leaf (status + tap to probe) --------------------------------

    fn render_storage_sd<D: DrawTarget<Color = Rgb565>>(
        &self,
        display: &mut D,
        data: &SystemData,
    ) {
        draw_header(display, data, "SD CARD", theme::SIGNAL);

        // Status: chamfered tag-labelled panel. Border + tag tint
        // tracks online/offline (green/signal). Read-only - the
        // button below triggers the probe.
        let (status_rect, probe_rect) = storage_sd_slots();
        let (accent, status_text) = if data.storage.sd_online {
            (theme::GREEN, "ONLINE")
        } else {
            (theme::SIGNAL, "NOT PRESENT")
        };
        chamfered_panel(display, status_rect, NOTCH, accent, 1);
        tag_label(
            display,
            status_rect.top_left.x,
            status_rect.top_left.y,
            "STATUS",
            accent,
            NOTCH,
        );
        fonts::draw_centered_in_rect(
            display, &fonts::value(),
            status_text, status_rect, accent,
        );

        // Probe action button (chamfered Primary), label depends on
        // state (initialize vs reprobe).
        let probe_text = if data.storage.sd_online { "REPROBE" } else { "INITIALIZE" };
        chamfered_button(
            display, probe_rect, probe_text,
            ButtonVariant::Primary, theme::SIGNAL,
        );
    }

    fn storage_sd_event(&mut self, event: &SystemEvent, _data: &mut SystemData) -> Action {
        match event {
            SystemEvent::Tap { x, y } if header_back_hit(*x, *y) => {
                self.view = SettingsView::Storage;
                Action::Redraw
            }
            SystemEvent::Swipe {
                dir: crate::events::SwipeDir::Right,
                region: crate::events::SwipeRegion::Content,
            } => {
                self.view = SettingsView::Storage;
                Action::Redraw
            }
            SystemEvent::Tap { x, y } => {
                let pt = Point::new(*x as i32, *y as i32);
                let (_, probe_rect) = storage_sd_slots();
                if probe_rect.contains(pt) {
                    return Action::InitSd;
                }
                Action::None
            }
            SystemEvent::StorageUsageUpdated { .. } => Action::Redraw,
            _ => Action::None,
        }
    }

    // -- Restore-from-SD leaf (destructive, gated on SD online) --------------

    fn render_storage_restore<D: DrawTarget<Color = Rgb565>>(
        &self,
        display: &mut D,
        data: &SystemData,
    ) {
        draw_header(display, data, "RESTORE FROM SD", theme::SIGNAL);

        // Warning panel: signal-bordered chamfered panel with a
        // RESTORE tag. Body explains what the action does.
        let (warn_rect, cancel_rect, primary_rect) = confirmation_slots();
        chamfered_panel(display, warn_rect, NOTCH, theme::SIGNAL, 1);
        tag_label(
            display,
            warn_rect.top_left.x,
            warn_rect.top_left.y,
            "RESTORE",
            theme::SIGNAL,
            NOTCH,
        );
        let body = if data.storage.sd_online {
            "FLASH CONFIG // REBOOT"
        } else {
            "SD NOT PRESENT"
        };
        let body_color = if data.storage.sd_online { theme::FG } else { theme::FG_DIM };
        fonts::draw_centered_in_rect(
            display, &fonts::body(),
            body, warn_rect, body_color,
        );

        // CANCEL / RESTORE buttons. Restore disabled (Ghost variant)
        // when SD isn't online.
        
        chamfered_button(
            display, cancel_rect, "CANCEL",
            ButtonVariant::Ghost, theme::STEEL,
        );
        if data.storage.sd_online {
            chamfered_button(
                display, primary_rect, "RESTORE",
                ButtonVariant::Primary, theme::SIGNAL,
            );
        } else {
            chamfered_button(
                display, primary_rect, "RESTORE",
                ButtonVariant::Ghost, theme::STEEL,
            );
        }
    }

    fn storage_restore_event(
        &mut self,
        event: &SystemEvent,
        data: &mut SystemData,
    ) -> Action {
        match event {
            SystemEvent::Tap { x, y } if header_back_hit(*x, *y) => {
                self.view = SettingsView::Storage;
                Action::Redraw
            }
            SystemEvent::Swipe {
                dir: crate::events::SwipeDir::Right,
                region: crate::events::SwipeRegion::Content,
            } => {
                self.view = SettingsView::Storage;
                Action::Redraw
            }
            SystemEvent::Tap { x, y } => {
                let pt = Point::new(*x as i32, *y as i32);
                let (_, cancel_rect, primary_rect) = confirmation_slots();
                if cancel_rect.contains(pt) {
                    self.view = SettingsView::Storage;
                    return Action::Redraw;
                }
                if primary_rect.contains(pt) && data.storage.sd_online {
                    // No bounce-back - the manager will software-
                    // reset shortly after this returns.
                    return Action::RestoreFromSd;
                }
                Action::None
            }
            SystemEvent::StorageUsageUpdated { .. } => Action::Redraw,
            _ => Action::None,
        }
    }

    // -- Factory reset leaf (destructive) ------------------------------------

    fn render_storage_factory_reset<D: DrawTarget<Color = Rgb565>>(
        &self,
        display: &mut D,
        data: &SystemData,
    ) {
        draw_header(display, data, "FACTORY RESET", theme::DANGER);

        // Warning panel: danger-tinted chamfered panel with PURGE
        // tag and irreversible-action copy.
        let (warn_rect, cancel_rect, primary_rect) = confirmation_slots();
        chamfered_panel(display, warn_rect, NOTCH, theme::DANGER, 1);
        tag_label(
            display,
            warn_rect.top_left.x,
            warn_rect.top_left.y,
            "PURGE",
            theme::DANGER,
            NOTCH,
        );
        fonts::draw_centered_in_rect(
            display, &fonts::body(),
            "WIPES CONFIG // LOGS", warn_rect, theme::FG,
        );

        // CANCEL (ghost) + PURGE (filled danger) button pair.
        
        chamfered_button(
            display, cancel_rect, "CANCEL",
            ButtonVariant::Ghost, theme::STEEL,
        );
        chamfered_button(
            display, primary_rect, "PURGE",
            ButtonVariant::Primary, theme::DANGER,
        );
    }

    fn storage_factory_reset_event(&mut self, event: &SystemEvent, _data: &mut SystemData) -> Action {
        match event {
            SystemEvent::Tap { x, y } if header_back_hit(*x, *y) => {
                self.view = SettingsView::Storage;
                Action::Redraw
            }
            SystemEvent::Swipe {
                dir: crate::events::SwipeDir::Right,
                region: crate::events::SwipeRegion::Content,
            } => {
                self.view = SettingsView::Storage;
                Action::Redraw
            }
            SystemEvent::Tap { x, y } => {
                let pt = Point::new(*x as i32, *y as i32);
                let (_, cancel_rect, primary_rect) = confirmation_slots();
                if cancel_rect.contains(pt) {
                    self.view = SettingsView::Storage;
                    return Action::Redraw;
                }
                if primary_rect.contains(pt) {
                    // Bounce back to Storage sub-index on confirm
                    // so the user sees the refreshed usage counts
                    // land naturally.
                    self.view = SettingsView::Storage;
                    return Action::FactoryReset;
                }
                Action::None
            }
            _ => Action::None,
        }
    }

    // -- Display sub-view ----------------------------------------------------

    fn render_display<D: DrawTarget<Color = Rgb565>>(
        &self,
        display: &mut D,
        data: &SystemData,
    ) {
        draw_header(display, data, "DISPLAY", theme::SIGNAL);

        let slots = display_slots();
        let always_on = data.config.display.always_on;

        // Brightness panel: tag-labelled chamfered panel with the
        // generic slider widget inside.
        let panel = slots.brightness_panel;
        chamfered_panel(display, panel, NOTCH, theme::SIGNAL, 1);
        tag_label(
            display,
            panel.top_left.x,
            panel.top_left.y,
            "BRIGHTNESS",
            theme::SIGNAL,
            NOTCH,
        );
        let pct = brightness_pct(data);
        let max_pct = data.config.display.max_brightness_pct();
        let mut label: String<8> = String::new();
        let _ = write!(label, "{:02}%", pct);
        slider(
            display, slots.brightness_bar,
            pct, BRIGHT_MIN_PCT, max_pct,
            Some(label.as_str()),
        );

        // Auto-lock panel: chamfered with 4 buttons inside. Selected
        // option = Primary, others = Ghost. When always_on is on,
        // ALL buttons render Ghost (and reject taps in display_event)
        // because the auto-lock timer is bypassed entirely - keeping
        // a Primary highlight would lie about what's active.
        let panel = slots.auto_lock_panel;
        chamfered_panel(display, panel, NOTCH, theme::SIGNAL, 1);
        tag_label(
            display,
            panel.top_left.x,
            panel.top_left.y,
            "AUTO-LOCK",
            theme::SIGNAL,
            NOTCH,
        );
        let current_secs = data.config.display.off_timeout_s as u32;
        for (i, opt) in AUTO_LOCK_OPTIONS.iter().enumerate() {
            let variant = if always_on || opt.secs != current_secs {
                ButtonVariant::Ghost
            } else {
                ButtonVariant::Primary
            };
            chamfered_button(
                display, slots.auto_lock_buttons[i], opt.label,
                variant, theme::SIGNAL,
            );
        }

        // NIGHT MODE + ALWAYS-ON: full-width toggle rows below the
        // panels. Reuses the same `row` widget the Settings index
        // uses so they read as list items rather than another panel.
        row(
            display, slots.night_mode_row,
            |d, cx, cy, c| glyphs::moon(d, cx, cy, 8, c),
            theme::CYAN,
            "NIGHT MODE",
            RowControl::Toggle(data.config.display.night_mode),
        );
        row(
            display, slots.always_on_row,
            |d, cx, cy, c| glyphs::power(d, cx, cy, 8, c),
            theme::CYAN,
            "ALWAYS-ON",
            RowControl::Toggle(always_on),
        );
    }

    fn display_event(&mut self, event: &SystemEvent, data: &mut SystemData) -> Action {
        match event {
            SystemEvent::Tap { x, y } if header_back_hit(*x, *y) => {
                self.view = SettingsView::Index;
                Action::Redraw
            }
            SystemEvent::Swipe {
                dir: crate::events::SwipeDir::Right,
                region: crate::events::SwipeRegion::Content,
            } => {
                self.view = SettingsView::Index;
                Action::Redraw
            }
            // Live brightness scrubbing - same idiom as Quick Access.
            SystemEvent::TouchPressed { x, y } => {
                let max_pct = data.config.display.max_brightness_pct();
                if let Some(v) = slider_value_from_x(
                    display_slots().brightness_bar,
                    *x as i32, *y as i32,
                    BRIGHT_MIN_PCT, max_pct,
                ) {
                    if v != brightness_pct(data) {
                        return Action::SetBrightness { percent: v };
                    }
                }
                Action::None
            }
            SystemEvent::Tap { x, y } => {
                let pt = Point::new(*x as i32, *y as i32);
                let slots = display_slots();
                let always_on = data.config.display.always_on;

                // Auto-lock buttons: rejected when always_on is on
                // (matches the Ghost rendering). Otherwise switch to
                // the tapped option.
                if !always_on {
                    for (i, opt) in AUTO_LOCK_OPTIONS.iter().enumerate() {
                        if slots.auto_lock_buttons[i].contains(pt) {
                            if data.config.display.off_timeout_s as u32 == opt.secs {
                                return Action::None; // already selected
                            }
                            return Action::SetAutoLock { secs: opt.secs };
                        }
                    }
                }

                // Toggle rows.
                if slots.night_mode_row.contains(pt) {
                    return Action::ToggleNightMode;
                }
                if slots.always_on_row.contains(pt) {
                    return Action::ToggleAlwaysOn;
                }
                Action::None
            }
            _ => Action::None,
        }
    }

    // -- Stub sub-views (Wifi / Bluetooth / Zigbee) ------------------------
    //
    // Placeholders so the Settings index can navigate to these rows
    // before their real contents land. Renders a grey tag-labeled
    // panel saying "WIP" with the view's title; back chevron and
    // right-swipe both pop to the index.

    fn render_stub<D: DrawTarget<Color = Rgb565>>(
        &self,
        display: &mut D,
        data: &SystemData,
        title: &str,
    ) {
        draw_header(display, data, title, theme::SIGNAL);
        let mut s = layout::VStack::new(LEAF_TOP_Y);
        let panel = s.slot(80);
        chamfered_panel(display, panel, NOTCH, theme::STEEL, 1);
        tag_label(
            display,
            panel.top_left.x,
            panel.top_left.y,
            "WIP",
            theme::STEEL,
            NOTCH,
        );
        fonts::draw_centered_in_rect(
            display, &fonts::value(),
            "TODO", panel, theme::FG_DIM,
        );
    }

    fn stub_event(&mut self, event: &SystemEvent, _data: &mut SystemData) -> Action {
        match event {
            SystemEvent::Tap { x, y } if header_back_hit(*x, *y) => {
                self.view = SettingsView::Index;
                Action::Redraw
            }
            SystemEvent::Swipe {
                dir: crate::events::SwipeDir::Right,
                region: crate::events::SwipeRegion::Content,
            } => {
                self.view = SettingsView::Index;
                Action::Redraw
            }
            _ => Action::None,
        }
    }
}

// -- Helpers -----------------------------------------------------------------

/// Pad a digit slice to `len` digits, right-aligned with leading zeros.
fn pad_digits(digits: &[u8], len: usize) -> [u8; MAX_DIGITS] {
    let mut p = [0u8; MAX_DIGITS];
    let offset = len.saturating_sub(digits.len());
    for (i, &d) in digits.iter().enumerate() {
        if offset + i < 8 {
            p[offset + i] = d;
        }
    }
    p
}


fn format_result(
    result: &SelfTestResult,
    unit: &'static str,
) -> (String<32>, Rgb565, Option<Rgb565>) {
    let mut buf: String<32> = String::new();
    match result {
        SelfTestResult::NotRun => {
            let _ = buf.push_str("--");
            (buf, theme::FG_DIM, None)
        }
        SelfTestResult::Running => {
            let _ = buf.push_str("RUNNING");
            (buf, theme::FG_MUTED, Some(theme::SIGNAL))
        }
        SelfTestResult::PassAxes3(v) => {
            let _ = write!(&mut buf, "{} {} {} {}", v[0], v[1], v[2], unit);
            (buf, theme::FG, Some(theme::GREEN))
        }
        SelfTestResult::FailAxes3(v) => {
            let _ = write!(&mut buf, "{} {} {} {}", v[0], v[1], v[2], unit);
            (buf, theme::DANGER, Some(theme::DANGER))
        }
        SelfTestResult::Error(_) => {
            let _ = buf.push_str("ERROR");
            (buf, theme::DANGER, Some(theme::DANGER))
        }
    }
}

// -- Sub-view layout helpers -------------------------------------------------
//
// Each non-trivial leaf sub-view has a `*_slots()` function that
// returns ALL its rects via a [`layout::VStack`] cursor. Render and
// event handlers both call the same `*_slots()` function and
// destructure into named rects, so they're guaranteed to agree on
// geometry - no chance of the event-side hit-test rect drifting from
// the render-side draw rect.

/// Top y for every leaf sub-view's first slot. Sits below the
/// header hairline with breathing room.
const LEAF_TOP_Y: i32 = ROWS_TOP + 18;

// -- IMU sub-view: per-test stacked (panel, button) pairs -------------------

/// One IMU test slot: `(panel_rect, run_button_rect)`.
type ImuSlot = (Rectangle, Rectangle);

/// IMU sub-view rects, indexed by test number. Each entry is a
/// `(panel, button)` pair so render and event loops can index by
/// test order.
fn imu_slots() -> [ImuSlot; 2] {
    let mut s = layout::VStack::new(LEAF_TOP_Y);
    let p0 = s.slot(80); s.gap(8); let b0 = s.slot(36);
    s.gap(16);
    let p1 = s.slot(80); s.gap(8); let b1 = s.slot(36);
    [(p0, b0), (p1, b1)]
}

// -- Storage SD: status panel + single full-width action button ------------

/// Storage-SD sub-view rects: (status_panel, action_button).
fn storage_sd_slots() -> (Rectangle, Rectangle) {
    let mut s = layout::VStack::new(LEAF_TOP_Y);
    let panel = s.slot(100);
    s.gap(18);
    let button = s.slot(36);
    (panel, button)
}

// -- Storage Restore / Factory Reset: panel + CANCEL/CONFIRM pair ----------

/// Restore / Factory-Reset sub-view rects: (warning_panel, cancel,
/// primary). Same layout for both; they differ only in colors and
/// labels.
fn confirmation_slots() -> (Rectangle, Rectangle, Rectangle) {
    let mut s = layout::VStack::new(LEAF_TOP_Y);
    let panel = s.slot(100);
    s.gap(18);
    let (cancel, primary) = s.pair(36, 12);
    (panel, cancel, primary)
}

// -- Display sub-view layout ----------------------------------------------

/// All rects the Display sub-view needs. Render and event handlers
/// both call [`display_slots`] and read the same fields, so geometry
/// can never drift between draw and hit-test.
struct DisplaySlots {
    /// Outer chamfered panel for the brightness section.
    brightness_panel: Rectangle,
    /// The slider trough inside `brightness_panel`. Same rect is
    /// passed to [`slider`] and [`slider_value_from_x`].
    brightness_bar: Rectangle,
    /// Outer chamfered panel for the auto-lock section.
    auto_lock_panel: Rectangle,
    /// One rect per [`AUTO_LOCK_OPTIONS`] entry, in order.
    auto_lock_buttons: [Rectangle; 4],
    night_mode_row: Rectangle,
    always_on_row: Rectangle,
}

fn display_slots() -> DisplaySlots {
    // Top section: margined VStack for the two chamfered panels.
    let mut top = layout::VStack::new(LEAF_TOP_Y);
    let brightness_panel = top.slot(70);
    top.gap(14);
    let auto_lock_panel = top.slot(80);
    top.gap(16);

    // Brightness slider hugs the bottom of its panel.
    let bar_y = brightness_panel.top_left.y
        + brightness_panel.size.height as i32 - SLIDER_BAR_H - 14;
    let brightness_bar = layout::VStack::inside(brightness_panel, 14, bar_y)
        .slot(SLIDER_BAR_H);

    // Auto-lock buttons: 4-up row hugging the bottom of the panel,
    // laid out via a sub-VStack scoped to the panel's interior.
    let btn_h = 30i32;
    let row_y = auto_lock_panel.top_left.y
        + auto_lock_panel.size.height as i32 - btn_h - 12;
    let mut inner = layout::VStack::inside(auto_lock_panel, 10, row_y);
    let auto_lock_buttons = inner.row::<4>(btn_h, 8);

    // Toggle rows below the panels: full-width, edge-to-edge, so
    // they read as list rows rather than floating panels - matches
    // the Settings index style.
    let mut rows = layout::VStack::with_margin(top.cursor_y(), 0);
    let night_mode_row = rows.slot(ROW_H);
    let always_on_row = rows.slot(ROW_H);

    DisplaySlots {
        brightness_panel,
        brightness_bar,
        auto_lock_panel,
        auto_lock_buttons,
        night_mode_row,
        always_on_row,
    }
}

/// Slider-percent view of the live brightness register (0..=255).
/// Mirrors the QA helper so both Display sub-view and QA agree on
/// the percent <-> register mapping.
fn brightness_pct(data: &SystemData) -> u8 {
    let hw = data.config.display.brightness_active as u16;
    ((hw * 100 / 255) as u8).clamp(BRIGHT_MIN_PCT, 100)
}

/// Pick the accent color (panel border + tag + result text) for a
/// given IMU test result. Visualises run state at a glance:
/// steel = inactive, signal = running, green = pass, danger =
/// fail/error.
fn imu_result_accent(result: &SelfTestResult) -> Rgb565 {
    match result {
        SelfTestResult::NotRun => theme::STEEL,
        SelfTestResult::Running => theme::SIGNAL,
        SelfTestResult::PassAxes3(_) => theme::GREEN,
        SelfTestResult::FailAxes3(_) | SelfTestResult::Error(_) => theme::DANGER,
    }
}

// -- Battery-graph helpers ---------------------------------------------------

/// Render the battery history as an edge-to-edge polyline inside
/// `rect`. Draws faint horizontal gridlines at 25/50/75% and
/// connects consecutive samples with short segments in the battery
/// color. Empty history gets a centered "NO DATA" caption.
///
/// `rect` is the full sparkline area (no surrounding card); the
/// polyline insets by a small horizontal margin so endpoints don't
/// land at the screen edge but is otherwise full width.
fn draw_battery_sparkline<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    rect: Rectangle,
    history: &crate::data::BatteryHistory,
) {
    // Small horizontal inset so the leftmost / rightmost samples
    // don't sit at the bezel arc; vertical inset is just visual
    // breathing room.
    const H_INSET: i32 = 24;
    const V_INSET: i32 = 6;
    let plot = Rectangle::new(
        Point::new(rect.top_left.x + H_INSET, rect.top_left.y + V_INSET),
        Size::new(
            (rect.size.width as i32 - 2 * H_INSET) as u32,
            (rect.size.height as i32 - 2 * V_INSET) as u32,
        ),
    );

    // Horizontal gridlines at 25 / 50 / 75 percent.
    let grid_style = PrimitiveStyle::with_stroke(theme::FG_DIM, 1);
    let left  = plot.top_left.x;
    let right = plot.top_left.x + plot.size.width as i32;
    for pct in [25, 50, 75] {
        let y = plot_y(pct, &plot);
        let _ = Line::new(Point::new(left, y), Point::new(right, y))
            .draw_styled(&grid_style, display);
    }

    // Empty state: centered caption.
    if history.is_empty() {
        fonts::draw_centered_in_rect(
            display, &fonts::body(), "NO DATA", plot, theme::FG_DIM,
        );
        return;
    }

    // Map each sample to a screen point, oldest on the left. When
    // there's only one sample the polyline has no segments - draw
    // a single-pixel dot via a length-1 Line so the view still
    // shows "there is data here."
    let n = history.len();
    let width = plot.size.width as i32;
    let sample_point = |i: usize, pct: u8| -> Point {
        let x = if n <= 1 {
            plot.top_left.x + width / 2
        } else {
            plot.top_left.x + (i as i32 * width) / (n as i32 - 1)
        };
        Point::new(x, plot_y(pct, &plot))
    };

    // Color each segment by the *lower* of its two endpoint
    // percents, so the line turns yellow / red the instant it drops
    // into a warning band. Matches the palette `battery_color` uses
    // for the battery icon elsewhere in the UI.
    let mut prev: Option<(Point, u8)> = None;
    for (i, sample) in history.iter().enumerate() {
        let p = sample_point(i, sample.percent);
        if let Some((q, prev_pct)) = prev {
            let color = crate::ui::primitives::battery_color(prev_pct.min(sample.percent));
            let stroke = PrimitiveStyle::with_stroke(color, 2);
            let _ = Line::new(q, p).draw_styled(&stroke, display);
        } else if n == 1 {
            // Single sample: draw it as a zero-length line so the
            // stroke still renders a small marker.
            let color = crate::ui::primitives::battery_color(sample.percent);
            let stroke = PrimitiveStyle::with_stroke(color, 2);
            let _ = Line::new(p, p).draw_styled(&stroke, display);
        }
        prev = Some((p, sample.percent));
    }
}

/// Map a battery percent (0-100, clamped) to the pixel Y inside
/// `plot`. 100% sits at the top edge, 0% at the bottom edge.
fn plot_y(percent: u8, plot: &Rectangle) -> i32 {
    let p = percent.min(100) as i32;
    let h = plot.size.height as i32;
    plot.top_left.y + (100 - p) * h / 100
}
