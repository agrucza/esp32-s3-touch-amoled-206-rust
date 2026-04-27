//! Alarm screen - rebuilt on the Nightwatch theme. Up to
//! [`MAX_ALARMS`] alarm entries.
//!
//! Three internal views:
//!
//! **List view** (default):
//! - Standard app chrome: status bar (yellow-tinted), Nightwatch
//!   header `ALARMS` + `×NN ACTIVE` live count, signal-red home
//!   indicator.
//! - Smooth-scrolling vertical list of chamfered alarm rows (one
//!   per entry, [`MAX_ALARMS`] total). Each row is yellow-bordered
//!   when enabled, steel-bordered when disabled, and shows: `HH:MM`
//!   left, the 7 day letters middle (yellow for active days, steel
//!   otherwise), enable toggle right. The full list scrolls under a
//!   clipped viewport - same drag pattern as the settings index
//!   ([`layout::ScrollState`] driven by `TouchPressed` / `TouchReleased`).
//! - Tap row body → open Edit. Tap toggle area → flip enabled.
//!
//! **Edit view**:
//! - Standard app chrome, header title `EDIT ALARM` + `ALM.0N`
//!   telemetry; chevron-back discards the in-flight edit (same as
//!   tapping CANCEL).
//! - Day selector row: 7 tappable cells with the day letter, yellow
//!   when active, FG_DIM when not. Tap toggles.
//! - HH:MM wheel picker (yellow accent). Drag a column to scroll;
//!   tap above/below the center band to step ±1. Hours and minutes
//!   both wrap at their range ends.
//! - `CANCEL | SET` action row at the bottom: Cancel returns to the
//!   list discarding edits; Set commits and persists.
//!
//! **Alert view** (alarm fired):
//! - Standard app chrome with `ALARM` title; chevron-back doubles
//!   as DISMISS for now.
//! - Readout panel flashing yellow↔signal-red, `RINGING` tag-label,
//!   hero-font HH:MM of the fired alarm centered inside.
//! - Action row: SNOOZE (Primary yellow) and DISMISS (Primary
//!   signal-red) chamfered_buttons.
//!
//! Will be replaced by a global notification overlay once that
//! infrastructure lands; for now the Alert view stays in-screen.

use core::fmt::Write;

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{Point, Size},
    pixelcolor::Rgb565,
    primitives::Rectangle,
};

use crate::events::SystemEvent;
use crate::ui::{fonts, layout, theme};
use crate::ui::types::{
    Action, AlarmEntry, Screen, SystemData, MAX_ALARMS,
};
use crate::ui::widgets::{
    action_row_rects, app_chrome_back_hit, chamfered_button, chamfered_panel,
    draw_app_chrome, fmt_2digit, handle_scroll_drag, render_action_row, render_scrolled,
    tag_label, toggle, ButtonVariant, Picker, Wheel,
    APP_CONTENT_TOP, APP_HOME_BAR_Y, NOTCH, TAG_LABEL_H,
    TOGGLE_H, TOGGLE_W, WHEEL_TOTAL_H,
};

// -- Constants ---------------------------------------------------------------

/// Per-screen accent. Yellow = wake/warn; mirrors the spec's
/// ALERTS-app tile border choice.
const ACCENT: Rgb565 = theme::YELLOW;

/// Static system-code prefix shown in the header's right-telemetry
/// slot during Edit (with the entry index).
const EDIT_TELEMETRY_PREFIX: &str = "ALM.";

/// Side margin matching readouts in stopwatch / timer / settings.
const SIDE_MARGIN: i32 = layout::VSTACK_SIDE_MARGIN;

/// Height of one alarm row in the List view.
const ROW_H: i32 = 80;

/// Vertical gap between alarm rows.
const ROW_GAP: i32 = 8;

/// Inner horizontal padding inside an alarm row.
const ROW_PAD_X: i32 = 16;

/// Top padding inside the scrollable list viewport so the first row
/// doesn't touch the header hairline.
const LIST_TOP_PAD: i32 = 4;

/// Vertical step from one row's top edge to the next.
const ROW_STEP: i32 = ROW_H + ROW_GAP;

// -- Edit view layout --------------------------------------------------------

/// Top y of the day-chip row.
const DAY_ROW_TOP: i32 = APP_CONTENT_TOP + 18;

/// Height of one day chip.
const DAY_CHIP_H: i32 = 24;

/// Horizontal gap between adjacent day chips.
const DAY_CHIP_GAP: i32 = 4;

/// Notch carved off TL+BR of each day chip - smaller than the
/// standard button notch since chips are about half the height.
const DAY_CHIP_NOTCH: i32 = 4;

/// Three-letter day labels for the Edit-view chip row, where each
/// chip is wide enough to hold the disambiguated label. Monday
/// first to mirror European / work-week convention.
const DAY_LABELS: [&str; 7] = ["MON", "TUE", "WED", "THU", "FRI", "SAT", "SUN"];

/// Single-letter day labels for the list overview. The list row
/// has to share its width with the time block and the toggle, so
/// three-letter labels won't fit there with readable gaps. Same
/// Monday-first order as [`DAY_LABELS`].
const DAY_LABELS_LIST: [&str; 7] = ["M", "T", "W", "T", "F", "S", "S"];

/// Map display index (0=Mon) to the bitmask bit position used by
/// `AlarmEntry::days` (0=Sun, 1=Mon, ..., 6=Sat).
const DAY_BIT: [u8; 7] = [1, 2, 3, 4, 5, 6, 0];

/// Top y of the HH:MM wheel picker. Picks up below the day chip
/// row with comfortable breathing room.
const PICKER_TOP: i32 = APP_CONTENT_TOP + 90;

/// Width of one wheel column.
const PICKER_COL_W: i32 = 80;

/// Horizontal gap between the HH and MM columns - wide enough to
/// fit the colon glyph at value-font size with no crowding.
const PICKER_GAP: i32 = 40;

/// Total horizontal extent of the two-column picker.
const PICKER_TOTAL_W: i32 = PICKER_COL_W * 2 + PICKER_GAP;

// -- Alert constants ---------------------------------------------------------

/// Readout panel height in the Alert view.
const ALERT_READOUT_H: i32 = 130;

/// Gap between the Alert readout and the action row.
const ALERT_READOUT_BUTTON_GAP: i32 = 8;

/// Y of the Alert readout's top edge - vertically centred between
/// the header bottom and the action row.
const ALERT_READOUT_TOP: i32 = APP_CONTENT_TOP
    + (layout::BOTTOM_TILE_Y - ALERT_READOUT_BUTTON_GAP - APP_CONTENT_TOP - ALERT_READOUT_H) / 2;

/// Ticks per alert-flash phase (250 ms at 20 Hz = 5 ticks).
const FLASH_PHASE_TICKS: u8 = 5;

// -- Views -------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AlarmView {
    List,
    Edit { index: usize },
    /// Alarm fired - shows the triggered alarm prominently. Tap
    /// SNOOZE / DISMISS / chevron to leave.
    Alert,
}

// -- Screen ------------------------------------------------------------------

pub struct AlarmScreen {
    view: AlarmView,
    /// Vertical scroll state for the alarm row list. Drag-driven
    /// via `TouchPressed` / `TouchReleased`, mirroring the settings
    /// index pattern.
    list_scroll: layout::ScrollState,
    /// HH:MM wheel picker for the Edit view. Wheels wrap; values
    /// are seeded from the entry on Edit entry and read back into
    /// the entry on Set.
    time_picker: Picker<2>,
    /// Days bitmask being edited (copy from entry on Edit entry,
    /// written back on Set).
    edit_days: u8,
    /// Tick counter for the alert flash, wraps freely.
    alert_ticks: u8,
}

impl AlarmScreen {
    pub fn new() -> Self {
        Self {
            view: AlarmView::List,
            list_scroll: layout::ScrollState::new(),
            time_picker: Picker::new([
                Wheel::new(0, 23, 0).with_wrap(true),
                Wheel::new(0, 59, 0).with_wrap(true),
            ]),
            edit_days: 0x7F,
            alert_ticks: 0,
        }
    }
}

impl Screen for AlarmScreen {
    fn render<D: DrawTarget<Color = Rgb565>>(&self, display: &mut D, data: &SystemData) {
        match self.view {
            AlarmView::List => self.render_list(display, data),
            AlarmView::Edit { index } => self.render_edit(display, data, index),
            AlarmView::Alert => self.render_alert(display, data),
        }
    }

    fn on_event(&mut self, event: &SystemEvent, data: &mut SystemData) -> Action {
        if matches!(event, SystemEvent::PowerButtonLong) {
            return Action::Shutdown;
        }

        // Alarm fired - check weekday and switch into Alert view.
        if matches!(event, SystemEvent::AlarmFired) {
            let weekday = day_of_week(
                data.time.year as i32,
                data.time.month as i32,
                data.time.day as i32,
            );
            let is_snooze = data.alarms.snoozed;
            data.alarms.snoozed = false;

            if is_snooze {
                data.alarms.alerting = true;
                self.view = AlarmView::Alert;
                self.alert_ticks = 0;
                return Action::StartBuzz { on_ms: 200, off_ms: 100 };
            }

            if let Some(idx) = data.alarms.active_hw {
                if data.alarms.entries[idx].fires_on(weekday) {
                    data.alarms.alerting = true;
                    self.view = AlarmView::Alert;
                    self.alert_ticks = 0;
                    return Action::StartBuzz { on_ms: 200, off_ms: 100 };
                }
            }
            return Action::None;
        }

        match self.view {
            AlarmView::List => self.list_event(event, data),
            AlarmView::Edit { index } => self.edit_event(event, data, index),
            AlarmView::Alert => self.alert_event(event, data),
        }
    }
}

// -- List view ---------------------------------------------------------------

impl AlarmScreen {
    fn render_list<D: DrawTarget<Color = Rgb565>>(
        &self, display: &mut D, data: &SystemData,
    ) {
        let active_count = data.alarms.entries.iter().filter(|e| e.enabled).count();
        let mut tele_buf: heapless::String<16> = heapless::String::new();
        let _ = write!(tele_buf, "x{:02} ACTIVE", active_count);
        draw_app_chrome(display, data, "ALARMS", tele_buf.as_str(), ACCENT);

        // Render the row stack inside a clipped viewport plus the
        // right-edge scroll indicator, both via the shared
        // `render_scrolled` helper so future scrollable screens
        // don't have to reimplement the clip + indicator pattern.
        render_scrolled(
            display,
            self.list_scroll.offset(),
            list_viewport_rect(),
            list_content_h(),
            ACCENT,
            |clipped, scroll| {
                for idx in 0..MAX_ALARMS {
                    self.render_row(clipped, data, idx, scroll);
                }
            },
        );
    }

    fn render_row<D: DrawTarget<Color = Rgb565>>(
        &self, display: &mut D, data: &SystemData, entry_idx: usize, scroll: i32,
    ) {
        let entry = &data.alarms.entries[entry_idx];
        let rect = row_rect(entry_idx, scroll);

        let border = if entry.enabled { ACCENT } else { theme::STEEL };
        chamfered_panel(display, rect, NOTCH, border, 1);

        // Time block, left-aligned.
        let time_color = if entry.enabled { ACCENT } else { theme::FG_DIM };
        let mut time_buf: heapless::String<8> = heapless::String::new();
        let _ = write!(time_buf, "{:02}:{:02}", entry.hour, entry.minute);
        let time_y = rect.top_left.y + (rect.size.height as i32 - 24) / 2;
        fonts::draw_at(
            display, &fonts::value(),
            time_buf.as_str(),
            rect.top_left.x + ROW_PAD_X,
            time_y,
            time_color,
        );

        // "NEXT" tag-label pinned to the row's TL chamfer when this
        // entry is the next one to fire.
        if entry.enabled && data.alarms.active_hw == Some(entry_idx) {
            tag_label(
                display,
                rect.top_left.x,
                rect.top_left.y,
                "NEXT",
                ACCENT,
                NOTCH,
            );
        }

        // Day-letter strip for the overview row: single-letter
        // labels in evenly-sized cells, centred between the time
        // block's right edge and the toggle's left edge. Three-letter
        // labels live in the Edit-view chip row, where there's room.
        let time_w = fonts::measure_width(&fonts::value(), time_buf.as_str());
        let time_right = rect.top_left.x + ROW_PAD_X + time_w;
        let toggle_left =
            rect.top_left.x + rect.size.width as i32 - ROW_PAD_X - TOGGLE_W;
        let day_cell_w: i32 = 24;
        let day_cell_h: i32 = 16;
        let n = DAY_LABELS_LIST.len() as i32;
        let strip_w = n * day_cell_w;
        let strip_x = (time_right + toggle_left - strip_w) / 2;
        let strip_y = rect.top_left.y + rect.size.height as i32 - 26;
        for (i, label) in DAY_LABELS_LIST.iter().enumerate() {
            let active = entry.enabled && (entry.days & (1 << DAY_BIT[i])) != 0;
            let cell_color = if active {
                ACCENT
            } else if entry.enabled {
                theme::STEEL
            } else {
                theme::STEEL_2
            };
            let cell_rect = Rectangle::new(
                Point::new(strip_x + i as i32 * day_cell_w, strip_y),
                Size::new(day_cell_w as u32, day_cell_h as u32),
            );
            fonts::draw_centered_in_rect(
                display, &fonts::caption(),
                label, cell_rect, cell_color,
            );
        }

        // Toggle switch, right-aligned.
        let toggle_x = rect.top_left.x + rect.size.width as i32 - ROW_PAD_X - TOGGLE_W;
        let toggle_y = rect.top_left.y + (rect.size.height as i32 - TOGGLE_H) / 2;
        toggle(display, Point::new(toggle_x, toggle_y), entry.enabled);
    }

    fn list_event(&mut self, event: &SystemEvent, data: &mut SystemData) -> Action {
        match event {
            SystemEvent::Tap { x, y } if app_chrome_back_hit(*x, *y) => {
                Action::Back
            }

            // Drag scroll routed through the shared scrollable
            // helper so screen code stays focused on hit-testing
            // rather than re-implementing scroll mechanics.
            SystemEvent::TouchPressed { .. } | SystemEvent::TouchReleased => {
                let viewport_h = list_viewport_rect().size.height as i32;
                if handle_scroll_drag(
                    &mut self.list_scroll, event, viewport_h, list_content_h(),
                ) {
                    return Action::Redraw;
                }
                Action::None
            }

            SystemEvent::Tap { x, y } => {
                let scroll = self.list_scroll.offset();
                let viewport = list_viewport_rect();
                let py = *y as i32;
                if py < viewport.top_left.y
                    || py >= viewport.top_left.y + viewport.size.height as i32
                {
                    return Action::None;
                }
                for idx in 0..MAX_ALARMS {
                    let rect = row_rect(idx, scroll);
                    if !rect_hit(rect, *x, *y) { continue; }

                    // Toggle hit zone: rightmost ~60 px so the user
                    // doesn't have to land precisely on the 32 px switch.
                    let toggle_zone_x =
                        rect.top_left.x + rect.size.width as i32 - 60;
                    if (*x as i32) >= toggle_zone_x {
                        data.alarms.entries[idx].enabled =
                            !data.alarms.entries[idx].enabled;
                        return Action::PersistAlarms;
                    }

                    // Body tap: open Edit. Seed the picker from
                    // the entry's HH:MM.
                    let entry = &data.alarms.entries[idx];
                    self.time_picker.wheels[0].set_value(entry.hour as i32);
                    self.time_picker.wheels[1].set_value(entry.minute as i32);
                    self.edit_days = entry.days;
                    self.view = AlarmView::Edit { index: idx };
                    return Action::Redraw;
                }
                Action::None
            }

            _ => Action::None,
        }
    }
}

// -- Edit view ---------------------------------------------------------------

impl AlarmScreen {
    fn render_edit<D: DrawTarget<Color = Rgb565>>(
        &self, display: &mut D, data: &SystemData, index: usize,
    ) {
        let mut tele_buf: heapless::String<8> = heapless::String::new();
        let _ = write!(tele_buf, "{}{:02}", EDIT_TELEMETRY_PREFIX, index);
        draw_app_chrome(display, data, "EDIT ALARM", tele_buf.as_str(), ACCENT);

        // Day chip row.
        let chips = day_chip_rects();
        for (i, rect) in chips.iter().enumerate() {
            let active = (self.edit_days & (1 << DAY_BIT[i])) != 0;
            day_chip(display, *rect, DAY_LABELS[i], active);
        }

        // HH:MM wheel picker.
        let cells = picker_cell_rects();
        self.time_picker.wheels[0].render(display, cells[0], ACCENT, fmt_2digit);
        self.time_picker.wheels[1].render(display, cells[1], ACCENT, fmt_2digit);

        // Colon between the two columns, sized to match the
        // wheel's center cell so it sits on the same baseline as
        // the selected HH and MM.
        let colon_cx = (cells[0].top_left.x + cells[0].size.width as i32
            + cells[1].top_left.x) / 2;
        let colon_cy = cells[0].top_left.y + cells[0].size.height as i32 / 2;
        let colon_rect = Rectangle::new(
            Point::new(colon_cx - 8, colon_cy - 16),
            Size::new(16, 32),
        );
        fonts::draw_centered_in_rect(
            display, &fonts::value(), ":", colon_rect, ACCENT,
        );

        // CANCEL | SET action row.
        render_action_row(display, ACCENT);
    }

    fn edit_event(
        &mut self, event: &SystemEvent, data: &mut SystemData, index: usize,
    ) -> Action {
        match event {
            // Header chevron: discard edit, return to list. Same as
            // tapping CANCEL.
            SystemEvent::Tap { x, y } if app_chrome_back_hit(*x, *y) => {
                self.view = AlarmView::List;
                Action::Redraw
            }

            SystemEvent::Tap { x, y } => {
                // Day chips first - small targets with the highest
                // mis-tap risk if the wheel below catches them.
                let chips = day_chip_rects();
                for (i, rect) in chips.iter().enumerate() {
                    if rect_hit(*rect, *x, *y) {
                        self.edit_days ^= 1 << DAY_BIT[i];
                        return Action::Redraw;
                    }
                }

                // Action row.
                let (cancel, set) = action_row_rects();
                if rect_hit(cancel, *x, *y) {
                    self.view = AlarmView::List;
                    return Action::Redraw;
                }
                if rect_hit(set, *x, *y) {
                    let h = self.time_picker.wheels[0].value() as u8;
                    let m = self.time_picker.wheels[1].value() as u8;
                    data.alarms.entries[index] = AlarmEntry {
                        hour: h,
                        minute: m,
                        days: self.edit_days,
                        enabled: true,
                    };
                    self.view = AlarmView::List;
                    return Action::PersistAlarms;
                }

                // Picker tap-step (above/below center band).
                let cells = picker_cell_rects();
                if self.time_picker.handle_event(event, &cells) {
                    return Action::Redraw;
                }
                Action::None
            }

            // Drag scroll on the wheels.
            SystemEvent::TouchPressed { .. } | SystemEvent::TouchReleased => {
                let cells = picker_cell_rects();
                if self.time_picker.handle_event(event, &cells) {
                    return Action::Redraw;
                }
                Action::None
            }

            _ => Action::None,
        }
    }
}

/// Rect for each of the 7 day chips, evenly split across the
/// content band with [`DAY_CHIP_GAP`] between them.
fn day_chip_rects() -> [Rectangle; 7] {
    let inner_w = theme::SCREEN_W as i32 - SIDE_MARGIN * 2;
    let chip_w = (inner_w - 6 * DAY_CHIP_GAP) / 7;
    core::array::from_fn(|i| {
        Rectangle::new(
            Point::new(
                SIDE_MARGIN + i as i32 * (chip_w + DAY_CHIP_GAP),
                DAY_ROW_TOP,
            ),
            Size::new(chip_w as u32, DAY_CHIP_H as u32),
        )
    })
}

/// Draw one day chip - chamfered border + matching label colour.
/// Active chips wear the accent; inactive chips read as steel.
fn day_chip<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, rect: Rectangle, label: &str, active: bool,
) {
    let color = if active { ACCENT } else { theme::STEEL_2 };
    chamfered_panel(display, rect, DAY_CHIP_NOTCH, color, 1);
    fonts::draw_centered_in_rect(display, &fonts::caption(), label, rect, color);
}

/// Per-column rects for the HH:MM wheel picker, centred horizontally.
fn picker_cell_rects() -> [Rectangle; 2] {
    let start_x = (theme::SCREEN_W as i32 - PICKER_TOTAL_W) / 2;
    [
        Rectangle::new(
            Point::new(start_x, PICKER_TOP),
            Size::new(PICKER_COL_W as u32, WHEEL_TOTAL_H as u32),
        ),
        Rectangle::new(
            Point::new(start_x + PICKER_COL_W + PICKER_GAP, PICKER_TOP),
            Size::new(PICKER_COL_W as u32, WHEEL_TOTAL_H as u32),
        ),
    ]
}

// -- Alert view --------------------------------------------------------------

impl AlarmScreen {
    fn render_alert<D: DrawTarget<Color = Rgb565>>(
        &self, display: &mut D, data: &SystemData,
    ) {
        draw_app_chrome(display, data, "ALARM", "RINGING", ACCENT);

        let phase = self.alert_ticks / FLASH_PHASE_TICKS;
        let panel_color = if phase % 2 == 0 { ACCENT } else { theme::DANGER };

        let panel = alert_readout_rect();
        chamfered_panel(display, panel, NOTCH, panel_color, 1);
        tag_label(
            display,
            panel.top_left.x,
            panel.top_left.y,
            "RINGING",
            panel_color,
            NOTCH,
        );

        let mut buf: heapless::String<8> = heapless::String::new();
        if let Some(idx) = data.alarms.active_hw {
            let entry = &data.alarms.entries[idx];
            let _ = write!(buf, "{:02}:{:02}", entry.hour, entry.minute);
        } else {
            let _ = buf.push_str("--:--");
        }
        let inner_rect = Rectangle::new(
            Point::new(
                panel.top_left.x,
                panel.top_left.y + TAG_LABEL_H,
            ),
            Size::new(
                panel.size.width,
                panel.size.height - TAG_LABEL_H as u32,
            ),
        );
        fonts::draw_centered_in_rect(
            display, &fonts::hero(),
            buf.as_str(), inner_rect, panel_color,
        );

        // Action row: SNOOZE (Primary yellow) + DISMISS (Primary signal).
        let [left, right] = layout::bottom_tile_row::<2>();
        chamfered_button(display, left, "SNOOZE", ButtonVariant::Primary, ACCENT);
        chamfered_button(display, right, "DISMISS", ButtonVariant::Primary, theme::SIGNAL);
    }

    fn alert_event(&mut self, event: &SystemEvent, data: &mut SystemData) -> Action {
        match event {
            // Flash animation tick (20 Hz IMU cadence).
            SystemEvent::MotionUpdated { .. } => {
                let old_phase = self.alert_ticks / FLASH_PHASE_TICKS;
                self.alert_ticks = self.alert_ticks.wrapping_add(1);
                let new_phase = self.alert_ticks / FLASH_PHASE_TICKS;
                if new_phase != old_phase { Action::Redraw } else { Action::None }
            }

            // Header chevron in Alert view doubles as DISMISS - the
            // alert isn't a navigable view, the chevron just gives a
            // visible affordance to leave.
            SystemEvent::Tap { x, y } if app_chrome_back_hit(*x, *y) => {
                data.alarms.alerting = false;
                Action::DismissAlarm
            }

            SystemEvent::Tap { x, y } => {
                let [left, right] = layout::bottom_tile_row::<2>();
                if rect_hit(left, *x, *y) {
                    data.alarms.alerting = false;
                    Action::SnoozeAlarm
                } else if rect_hit(right, *x, *y) {
                    data.alarms.alerting = false;
                    Action::DismissAlarm
                } else {
                    Action::None
                }
            }

            _ => Action::None,
        }
    }
}

// -- Helpers -----------------------------------------------------------------

/// Rect for the alarm entry at `idx` inside the List view's
/// scrollable area, shifted by the current scroll offset. `scroll = 0`
/// gives natural positioning; positive `scroll` moves rows upward as
/// the user pulls up.
fn row_rect(idx: usize, scroll: i32) -> Rectangle {
    let y = APP_CONTENT_TOP + LIST_TOP_PAD + idx as i32 * ROW_STEP - scroll;
    Rectangle::new(
        Point::new(SIDE_MARGIN, y),
        Size::new(
            (theme::SCREEN_W as i32 - SIDE_MARGIN * 2) as u32,
            ROW_H as u32,
        ),
    )
}

/// Visible viewport rect for the List view. Spans from just below
/// the header hairline to just above the home-indicator bar; the row
/// list renders into a clipped sub-target of this rect so off-screen
/// rows are hardware-clipped.
fn list_viewport_rect() -> Rectangle {
    let top = APP_CONTENT_TOP;
    let bot = APP_HOME_BAR_Y - 4;
    Rectangle::new(
        Point::new(0, top),
        Size::new(theme::SCREEN_W as u32, (bot - top) as u32),
    )
}

/// Total content height of the List view: every alarm row plus the
/// inter-row gaps and a small pad above and below.
fn list_content_h() -> i32 {
    LIST_TOP_PAD
        + MAX_ALARMS as i32 * ROW_STEP
        - ROW_GAP
        + LIST_TOP_PAD
}

/// Rect for the readout panel in the Alert view.
fn alert_readout_rect() -> Rectangle {
    Rectangle::new(
        Point::new(SIDE_MARGIN, ALERT_READOUT_TOP),
        Size::new(
            (theme::SCREEN_W as i32 - SIDE_MARGIN * 2) as u32,
            ALERT_READOUT_H as u32,
        ),
    )
}

fn rect_hit(rect: Rectangle, x: u16, y: u16) -> bool {
    let px = x as i32;
    let py = y as i32;
    let rx = rect.top_left.x;
    let ry = rect.top_left.y;
    px >= rx
        && px < rx + rect.size.width as i32
        && py >= ry
        && py < ry + rect.size.height as i32
}

/// Day of week via Zeller's congruence, returning 0=Sunday..6=Saturday.
pub fn day_of_week(year: i32, month: i32, day: i32) -> u8 {
    let (m, y) = if month < 3 { (month + 12, year - 1) } else { (month, year) };
    let k = y % 100;
    let j = y / 100;
    let h = (day + (13 * (m + 1)) / 5 + k + k / 4 + j / 4 + 5 * j) % 7;
    // Zeller: h=0=Saturday, h=1=Sunday, ...
    // We want: 0=Sunday, 6=Saturday
    ((h + 6) % 7) as u8
}
