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
    draw_target::DrawTarget,
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
    card, chamfered_panel, header, header_icon_hit, home_indicator, row, status_bar,
    tag_label, value_body, CardStyle, RowControl,
    Numpad, NumpadAction, NOTCH, ROW_H, STATUS_BAR_H, MAX_DIGITS,
};

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

/// Rect for the Nth row in the settings Index / Storage sub-index.
///
/// Rows span the full screen width; the `row` widget's internal
/// `ROW_PAD` keeps the icon, label, and hairline visually inset from
/// the bezel arc on its own. Letting the rect go edge-to-edge means
/// the row reads as a real list row rather than a floating panel.
fn row_rect(index: usize) -> Rectangle {
    let y = ROWS_TOP + index as i32 * ROW_H;
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
    }
}

struct IndexRow {
    label: &'static str,
    icon: RowIcon,
    value_fn: fn(&SystemData) -> String<20>,
    target: SettingsView,
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

const STORAGE_INDEX_ROWS: &[IndexRow] = &[
    IndexRow {
        label: "FLASH",
        icon: RowIcon::Flash,
        value_fn: storage_flash_value,
        target: SettingsView::StorageFlash,
    },
    IndexRow {
        label: "SD CARD",
        icon: RowIcon::SdCard,
        value_fn: storage_sd_value,
        target: SettingsView::StorageSd,
    },
    IndexRow {
        label: "RESTORE FROM SD",
        icon: RowIcon::Restore,
        value_fn: storage_restore_value,
        target: SettingsView::StorageRestoreFlash,
    },
    IndexRow {
        label: "FACTORY RESET",
        icon: RowIcon::Skull,
        value_fn: storage_reset_value,
        target: SettingsView::StorageFactoryReset,
    },
];

const INDEX_ROWS: &[IndexRow] = &[
    IndexRow {
        label: "CLOCK",
        icon: RowIcon::Clock,
        value_fn: clock_value,
        target: SettingsView::Clock,
    },
    IndexRow {
        label: "BATTERY",
        icon: RowIcon::Battery,
        value_fn: battery_value,
        target: SettingsView::Battery,
    },
    IndexRow {
        label: "6-AXIS IMU",
        icon: RowIcon::Imu,
        value_fn: imu_value,
        target: SettingsView::Imu,
    },
    IndexRow {
        label: "STORAGE",
        icon: RowIcon::Storage,
        value_fn: storage_value,
        target: SettingsView::Storage,
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
}

impl SettingsScreen {
    pub fn new() -> Self {
        Self {
            view: SettingsView::Index,
            numpad: Numpad::new(6),
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
        }
    }

    fn on_event(&mut self, event: &SystemEvent, data: &mut SystemData) -> Action {
        if matches!(event, SystemEvent::PowerButtonLong) {
            return Action::Shutdown;
        }

        match self.view {
            SettingsView::Index => self.index_event(event),
            SettingsView::Imu => self.imu_event(event),
            SettingsView::Clock => self.clock_event(event, data),
            SettingsView::TimeEntry => self.time_entry_event(event, data),
            SettingsView::DateEntry => self.date_entry_event(event, data),
            SettingsView::Battery => self.battery_event(event),
            SettingsView::Storage => self.storage_index_event(event),
            SettingsView::StorageFlash => self.storage_flash_event(event),
            SettingsView::StorageSd => self.storage_sd_event(event),
            SettingsView::StorageRestoreFlash => self.storage_restore_event(event, data),
            SettingsView::StorageFactoryReset => self.storage_factory_reset_event(event),
        }
    }
}

// -- Index sub-view ----------------------------------------------------------

impl SettingsScreen {
    fn render_index<D: DrawTarget<Color = Rgb565>>(
        &self, display: &mut D, data: &SystemData,
    ) {
        draw_header(display, data, "SETTINGS", theme::SIGNAL);
        render_rows(display, data, INDEX_ROWS);

        // Night Mode toggle row, backed by `data.config.display.night_mode`.
        // Tapping fires `Action::ToggleNightMode` so the Model flips
        // the config, clamps brightness if needed, and persists on
        // the next `TouchReleased`.
        let rect = row_rect(INDEX_ROWS.len());
        row(
            display, rect,
            |d, cx, cy, c| draw_row_icon(d, RowIcon::NightMode, cx, cy, c),
            theme::CYAN,
            "NIGHT MODE",
            RowControl::Toggle(data.config.display.night_mode),
        );
    }

    fn index_event(&mut self, event: &SystemEvent) -> Action {
        match event {
            SystemEvent::Tap { x, y } if header_back_hit(*x, *y) => {
                Action::Back
            }
            SystemEvent::Tap { x, y } => {
                if let Some(target) = row_hit(*x, *y, INDEX_ROWS) {
                    self.view = target;
                    return Action::Redraw;
                }
                // Night Mode row lives at index INDEX_ROWS.len().
                let pt = Point::new(*x as i32, *y as i32);
                if row_rect(INDEX_ROWS.len()).contains(pt) {
                    return Action::ToggleNightMode;
                }
                Action::None
            }
            _ => Action::None,
        }
    }
}

// -- Shared row rendering / hit-testing for index + storage sub-index --------

/// Render a stack of [`IndexRow`]s using `nightwatch::row`. The right
/// control is an inline mono value per row; empty strings collapse
/// into no inline text (bare row) so the row reads as a drill-in
/// without redundant chevron noise.
fn render_rows<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, data: &SystemData, rows: &[IndexRow],
) {
    for (i, r) in rows.iter().enumerate() {
        let rect = row_rect(i);
        let val = (r.value_fn)(data);
        let kind = r.icon;
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
}

/// Row hit test: if `(x, y)` lands in any row's rect, return that
/// row's target view.
fn row_hit(x: u16, y: u16, rows: &[IndexRow]) -> Option<SettingsView> {
    let pt = Point::new(x as i32, y as i32);
    for (i, r) in rows.iter().enumerate() {
        if row_rect(i).contains(pt) {
            return Some(r.target);
        }
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

    fn clock_event(&mut self, event: &SystemEvent, data: &SystemData) -> Action {
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

        for (i, test) in IMU_TESTS.iter().enumerate() {
            let rect = layout::content_card_rect(i);
            let result = data.self_tests[test.id as usize];

            let (value_buf, value_color, dot) = format_result(&result, test.unit);
            let style = match dot {
                Some(color) => CardStyle::DEFAULT.with_status_dot(color),
                None => CardStyle::DEFAULT,
            };

            if matches!(result, SelfTestResult::Running) {
                card(display, rect, dimmed(style));
            } else {
                card(display, rect, style);
            }

            value_body(display, rect, test.label, value_buf.as_str(), value_color);
        }
    }

    fn imu_event(&mut self, event: &SystemEvent) -> Action {
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
                for (i, test) in IMU_TESTS.iter().enumerate() {
                    if !layout::content_card_rect(i)
                        .contains(Point::new(*x as i32, *y as i32))
                    {
                        continue;
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

        // Status card: current percent + voltage (from the live
        // PowerData snapshot, not from the history). History is for
        // trend; this line is "what is it right now."
        let status_rect = layout::content_card_rect(0);
        card(display, status_rect, CardStyle::DEFAULT);
        let mut val: String<20> = String::new();
        match (data.power.battery_percent, data.power.battery_voltage_mv) {
            (Some(pct), Some(mv)) => {
                let _ = write!(val, "{}% / {}.{:02}V", pct, mv / 1000, (mv % 1000) / 10);
            }
            (Some(pct), None) => { let _ = write!(val, "{}%", pct); }
            _                  => { let _ = val.push_str("--"); }
        }
        value_body(display, status_rect, "NOW", val.as_str(), theme::FG);

        // Graph panel below the status card. Custom geometry: a
        // single wide rect that holds gridlines + polyline. Height
        // picked to fit the remaining content band without pushing
        // into the bezel corners.
        let g = graph_rect();
        card(display, g, CardStyle::DEFAULT);
        draw_battery_graph(display, g, &data.battery_history);
    }

    fn battery_event(&mut self, event: &SystemEvent) -> Action {
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
        render_rows(display, data, STORAGE_INDEX_ROWS);
    }

    fn storage_index_event(&mut self, event: &SystemEvent) -> Action {
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
                if let Some(target) = row_hit(*x, *y, STORAGE_INDEX_ROWS) {
                    self.view = target;
                    return Action::Redraw;
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

    fn storage_flash_event(&mut self, event: &SystemEvent) -> Action {
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

        // Card 0: status line. Green dot when online, yellow when
        // not. Read-only; the button below triggers the probe.
        let status_rect = layout::content_card_rect(0);
        let (dot, status_text, status_color) = if data.storage.sd_online {
            (theme::GREEN, "ONLINE",      theme::FG)
        } else {
            (theme::SIGNAL, "NOT PRESENT", theme::SIGNAL)
        };
        card(display, status_rect, CardStyle::DEFAULT.with_status_dot(dot));
        value_body(display, status_rect, "STATUS", status_text, status_color);

        // Card 1: tap target to (re-)probe.
        let probe_rect = layout::content_card_rect(1);
        card(display, probe_rect, CardStyle::DEFAULT);
        let probe_text = if data.storage.sd_online { "TAP TO REPROBE" } else { "TAP TO INITIALIZE" };
        value_body(display, probe_rect, "PROBE", probe_text, theme::FG);
    }

    fn storage_sd_event(&mut self, event: &SystemEvent) -> Action {
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
                if layout::content_card_rect(1).contains(pt) {
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

        // Card 0: summary of what happens. Always present so the
        // user has context whether SD is online or not.
        let warn_rect = layout::content_card_rect(0);
        card(display, warn_rect, CardStyle::DEFAULT.with_status_dot(theme::SIGNAL));
        value_body(
            display,
            warn_rect,
            "OVERWRITES",
            "FLASH CONFIG + REBOOT",
            theme::SIGNAL,
        );

        // Card 1: confirm target. Disabled (dimmed, different label)
        // when the SD mirror isn't online - matches the InitSd row's
        // gating idea but flipped: here offline is the blocker.
        let confirm_rect = layout::content_card_rect(1);
        if data.storage.sd_online {
            card(display, confirm_rect, CardStyle::DEFAULT.with_status_dot(theme::SIGNAL));
            value_body(
                display,
                confirm_rect,
                "CONFIRM",
                "TAP TO RESTORE",
                theme::SIGNAL,
            );
        } else {
            card(display, confirm_rect, dimmed(CardStyle::DEFAULT));
            value_body(
                display,
                confirm_rect,
                "CONFIRM",
                "SD NOT PRESENT",
                theme::FG_DIM,
            );
        }
    }

    fn storage_restore_event(
        &mut self,
        event: &SystemEvent,
        data: &SystemData,
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
                if !data.storage.sd_online {
                    return Action::None;
                }
                let pt = Point::new(*x as i32, *y as i32);
                if layout::content_card_rect(1).contains(pt) {
                    // No bounce-back - the manager will software-
                    // reset shortly after this returns, so the
                    // next frame never draws. Leaving `view` on
                    // StorageRestoreFlash is fine.
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

        // Card 0: warning summary (read-only).
        let warn_rect = layout::content_card_rect(0);
        card(display, warn_rect, CardStyle::DEFAULT.with_status_dot(theme::DANGER));
        value_body(
            display,
            warn_rect,
            "WARNING",
            "WIPES CONFIG + LOGS",
            theme::DANGER,
        );

        // Card 1: confirmation tap target.
        let confirm_rect = layout::content_card_rect(1);
        card(display, confirm_rect, CardStyle::DEFAULT.with_status_dot(theme::DANGER));
        value_body(
            display,
            confirm_rect,
            "CONFIRM",
            "TAP TO WIPE",
            theme::DANGER,
        );
    }

    fn storage_factory_reset_event(&mut self, event: &SystemEvent) -> Action {
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
                if layout::content_card_rect(1).contains(pt) {
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

fn dimmed(mut style: CardStyle) -> CardStyle {
    style.bg = theme::FG_DIM;
    style
}

// -- Battery-graph helpers ---------------------------------------------------

/// Bounding rect for the battery-history graph panel. Sits below
/// the status card in the battery sub-view, centered in the same
/// horizontal band as the card stack.
fn graph_rect() -> Rectangle {
    // Start one card-slot below the status card (content_card_rect(0)),
    // stretch down to the bezel-safe content bottom so the graph
    // has the whole remaining screen height to itself.
    let top = layout::content_card_rect(1).top_left.y;
    let bot = theme::CONTENT_BOTTOM;
    Rectangle::new(
        Point::new(layout::CARD_MARGIN_X, top),
        Size::new(layout::CARD_WIDTH as u32, (bot - top) as u32),
    )
}

/// Render the battery history as a polyline inside `rect`. Draws
/// faint horizontal gridlines at 25/50/75% and connects consecutive
/// samples with short line segments in the battery color. Empty history gets a
/// centered "NO DATA" caption instead.
fn draw_battery_graph<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    rect: Rectangle,
    history: &crate::data::BatteryHistory,
) {
    // Inset the drawable area so strokes don't kiss the card border.
    const INSET: i32 = 10;
    let plot = Rectangle::new(
        Point::new(rect.top_left.x + INSET, rect.top_left.y + INSET),
        Size::new(
            (rect.size.width as i32 - 2 * INSET) as u32,
            (rect.size.height as i32 - 2 * INSET) as u32,
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
