//! Alarm screen - manage up to 8 alarms.
//!
//! **List view** (default):
//! - Header bar: Close (X) + "ALARMS"
//! - Scrollable card list showing each alarm entry with time,
//!   day indicators, and enabled/disabled state
//! - Tap a card to edit it, tap the enable dot to toggle
//!
//! **Edit view** (numpad):
//! - Header bar: Back + "EDIT ALARM"
//! - Time label showing HH:MM as entered
//! - Day selector row (S M T W T F S) with toggle taps
//! - Numpad for entering the time
//!
//! The alarm list lives in `SystemData.alarms`. When an alarm is
//! added/changed/toggled, the screen computes the next alarm to
//! fire and programs it into the RTC hardware via `Action::SetAlarm`.

use core::fmt::Write;

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::Point,
    pixelcolor::Rgb565,
};

use crate::events::SystemEvent;
use crate::ui::{fonts, glyphs, layout, primitives, theme};
use crate::ui::types::{
    Action, AlarmEntry, Screen, SystemData, MAX_ALARMS,
};
use crate::ui::widgets::{
    card, header_bar, icon_button, page_scrollbar, value_body, CardStyle,
    HeaderIcon, Numpad, NumpadAction, MAX_DIGITS,
};

// -- Constants ---------------------------------------------------------------

const NUMPAD_TIME_Y: i32 = 90;
const DAY_ROW_Y: i32 = 128;
const DAY_LABELS: [&str; 7] = ["M", "T", "W", "T", "F", "S", "S"];
/// Maps display index (0=Mon) to bitmask bit (0=Sun, 1=Mon, ...).
const DAY_BIT: [u8; 7] = [1, 2, 3, 4, 5, 6, 0];
const DAY_SPACING: i32 = 48;
#[allow(dead_code)]
const DAY_DOT_RADIUS: i32 = 16;

/// How many alarm cards are visible per page.
const CARDS_PER_PAGE: usize = 4;

/// Snooze duration in minutes.
#[allow(dead_code)]
const SNOOZE_MINUTES: u8 = 10;

/// Ticks per alert flash phase (250ms at 20 Hz = 5 ticks).
const FLASH_PHASE_TICKS: u8 = 5;

// -- Views -------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AlarmView {
    List,
    Edit { index: usize },
    /// Alarm fired - shows the triggered alarm prominently.
    /// Tap anywhere to dismiss.
    Alert,
}

// -- Screen ------------------------------------------------------------------

pub struct AlarmScreen {
    view: AlarmView,
    /// Which page of the alarm list (0-indexed).
    page: usize,
    numpad: Numpad,
    /// Days bitmask being edited (copy from entry, written back on confirm).
    edit_days: u8,
    /// Tick counter for the alert flash (wrapping).
    alert_ticks: u8,
}

impl AlarmScreen {
    pub fn new() -> Self {
        Self {
            view: AlarmView::List,
            page: 0,
            numpad: Numpad::new(4).with_top(165),
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

        // Alarm fired - check weekday and show alert.
        if matches!(event, SystemEvent::AlarmFired) {
            let weekday = day_of_week(
                data.time.year as i32,
                data.time.month as i32,
                data.time.day as i32,
            );
            let is_snooze = data.alarms.snoozed;
            data.alarms.snoozed = false;

            if is_snooze {
                // Snooze expired - alert again.
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
            // Wrong day - the TimeUpdated poll will reprogram.
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
        header_bar(
            display,
            layout::header_rect(),
            HeaderIcon::Close,
            "ALARMS",
            theme::AMBER,
        );

        let start = self.page * CARDS_PER_PAGE;
        for i in 0..CARDS_PER_PAGE {
            let idx = start + i;
            if idx >= MAX_ALARMS { break; }
            let entry = &data.alarms.entries[idx];
            let rect = layout::content_card_rect(i);

            let dot = if entry.enabled {
                theme::GREEN
            } else {
                theme::RED
            };
            let style = CardStyle::DEFAULT.with_status_dot(dot);
            card(display, rect, style);

            // Format: "HH:MM" as value, day string as label.
            let mut time_buf: heapless::String<8> = heapless::String::new();
            let _ = write!(time_buf, "{:02}:{:02}", entry.hour, entry.minute);

            let mut day_buf: heapless::String<26> = heapless::String::new();
            if entry.enabled {
                if data.alarms.active_hw == Some(idx) {
                    let _ = day_buf.push_str("NEXT: ");
                }
                format_days(entry.days, &mut day_buf);
            } else {
                let _ = day_buf.push_str("DISABLED");
            }

            value_body(display, rect, day_buf.as_str(), time_buf.as_str(),
                if entry.enabled { theme::TEXT_WHITE } else { theme::TEXT_MUTED });
        }

        let pages = (MAX_ALARMS + CARDS_PER_PAGE - 1) / CARDS_PER_PAGE;
        page_scrollbar(display, pages, self.page);
    }

    fn list_event(&mut self, event: &SystemEvent, data: &mut SystemData) -> Action {
        match event {
            SystemEvent::Tap { x, y } if layout::header_icon_hit(*x, *y) => {
                Action::Back
            }

            // Up/down swipes for pagination.
            SystemEvent::Swipe {
                dir: crate::events::SwipeDir::Up,
                region: crate::events::SwipeRegion::Content,
            } => {
                let pages = (MAX_ALARMS + CARDS_PER_PAGE - 1) / CARDS_PER_PAGE;
                self.page = (self.page + 1) % pages;
                Action::Redraw
            }
            SystemEvent::Swipe {
                dir: crate::events::SwipeDir::Down,
                region: crate::events::SwipeRegion::Content,
            } => {
                let pages = (MAX_ALARMS + CARDS_PER_PAGE - 1) / CARDS_PER_PAGE;
                self.page = (self.page + pages - 1) % pages;
                Action::Redraw
            }

            SystemEvent::Tap { x, y } => {
                let start = self.page * CARDS_PER_PAGE;
                for i in 0..CARDS_PER_PAGE {
                    let idx = start + i;
                    if idx >= MAX_ALARMS { break; }
                    let rect = layout::content_card_rect(i);
                    if rect.contains(Point::new(*x as i32, *y as i32)) {
                        // Right side tap = toggle enabled.
                        if *x as i32 > rect.top_left.x + rect.size.width as i32 - 60 {
                            data.alarms.entries[idx].enabled =
                                !data.alarms.entries[idx].enabled;
                            return Action::Redraw;
                        }
                        // Left/center tap = edit.
                        let entry = &data.alarms.entries[idx];
                        self.numpad.prefill(&[
                            entry.hour / 10, entry.hour % 10,
                            entry.minute / 10, entry.minute % 10,
                        ]);
                        self.edit_days = entry.days;
                        self.view = AlarmView::Edit { index: idx };
                        return Action::Redraw;
                    }
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
        &self, display: &mut D, _data: &SystemData, _index: usize,
    ) {
        header_bar(
            display,
            layout::header_rect(),
            HeaderIcon::Back,
            "EDIT ALARM",
            theme::AMBER,
        );

        // HH:MM label from digits.
        let mut padded = [0u8; MAX_DIGITS];
        let offset = 4usize.saturating_sub(self.numpad.digits.len());
        for (i, &d) in self.numpad.digits.iter().enumerate() {
            padded[offset + i] = d;
        }
        let mut buf: heapless::String<8> = heapless::String::new();
        let _ = write!(buf, "{}{}:{}{}", padded[0], padded[1], padded[2], padded[3]);
        fonts::draw_centered(
            display, &fonts::value(),
            &buf,
            theme::SCREEN_W as i32 / 2, NUMPAD_TIME_Y,
            theme::AMBER,
        );

        // Day selector row.
        let total_w = 7 * DAY_SPACING;
        let start_x = (theme::SCREEN_W as i32 - total_w) / 2 + DAY_SPACING / 2;
        for (i, label) in DAY_LABELS.iter().enumerate() {
            let cx = start_x + i as i32 * DAY_SPACING;
            let active = (self.edit_days & (1 << DAY_BIT[i])) != 0;
            let color = if active { theme::AMBER } else { theme::TEXT_MUTED };
            fonts::draw_centered(
                display, &fonts::body(),
                label, cx, DAY_ROW_Y,
                color,
            );
        }

        // Numpad.
        self.numpad.render(display);
    }

    fn edit_event(
        &mut self, event: &SystemEvent, data: &mut SystemData, index: usize,
    ) -> Action {
        match event {
            // Back: discard changes.
            SystemEvent::Tap { x, y } if layout::header_icon_hit(*x, *y) => {
                self.view = AlarmView::List;
                Action::Redraw
            }

            SystemEvent::Tap { x, y } => {
                // Check day selector row taps.
                let total_w = 7 * DAY_SPACING;
                let start_x = (theme::SCREEN_W as i32 - total_w) / 2 + DAY_SPACING / 2;
                let py = *y as i32;
                let px = *x as i32;
                if py >= DAY_ROW_Y - 10 && py <= DAY_ROW_Y + 20 {
                    for i in 0..7 {
                        let cx = start_x + i as i32 * DAY_SPACING;
                        if (px - cx).abs() < DAY_SPACING / 2 {
                            self.edit_days ^= 1 << DAY_BIT[i as usize];
                            return Action::Redraw;
                        }
                    }
                }

                // Numpad taps.
                if let Some(action) = self.numpad.hit_test(*x, *y) {
                    match action {
                        NumpadAction::Confirm => {
                            let mut padded = [0u8; MAX_DIGITS];
                            let offset = 4usize.saturating_sub(self.numpad.digits.len());
                            for (i, &d) in self.numpad.digits.iter().enumerate() {
                                padded[offset + i] = d;
                            }
                            let h = padded[0] * 10 + padded[1];
                            let m = padded[2] * 10 + padded[3];
                            if h < 24 && m < 60 {
                                data.alarms.entries[index] = AlarmEntry {
                                    hour: h,
                                    minute: m,
                                    days: self.edit_days,
                                    enabled: true,
                                };
                                self.view = AlarmView::List;
                                return Action::Redraw;
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

// -- Alert view --------------------------------------------------------------

impl AlarmScreen {
    fn render_alert<D: DrawTarget<Color = Rgb565>>(
        &self, display: &mut D, data: &SystemData,
    ) {
        header_bar(
            display,
            layout::header_rect(),
            HeaderIcon::None,
            "ALARM",
            theme::AMBER,
        );

        // Hero pill with the alarm time, flashing amber/red.
        let phase = self.alert_ticks / FLASH_PHASE_TICKS;
        let pill_color = if phase % 2 == 0 { theme::AMBER } else { theme::RED };
        primitives::pill_solid(
            display,
            layout::HERO_PILL_X, layout::HERO_PILL_Y,
            layout::HERO_PILL_W, layout::HERO_PILL_H,
            pill_color,
        );

        // Show the fired alarm's time in the pill.
        let mut buf: heapless::String<8> = heapless::String::new();
        if let Some(idx) = data.alarms.active_hw {
            let entry = &data.alarms.entries[idx];
            let _ = write!(buf, "{:02}:{:02}", entry.hour, entry.minute);
        } else {
            let _ = buf.push_str("--:--");
        }
        fonts::draw_centered_in_rect(
            display, &fonts::hero(),
            &buf, layout::HERO_RECT,
            theme::BG,
        );

        // Left circle: snooze.
        icon_button(
            display,
            layout::LEFT_CIRCLE_CX, layout::CIRCLE_CY,
            theme::PANEL_BG,
            glyphs::hourglass, theme::TEXT_WHITE,
            "SNOOZE", theme::TEXT_DIM,
        );

        // Right circle: dismiss.
        icon_button(
            display,
            layout::RIGHT_CIRCLE_CX, layout::CIRCLE_CY,
            theme::PANEL_BG,
            glyphs::stop, theme::TEXT_WHITE,
            "DISMISS", theme::TEXT_DIM,
        );
    }

    fn alert_event(&mut self, event: &SystemEvent, data: &mut SystemData) -> Action {
        match event {
            // Flash animation tick.
            SystemEvent::MotionUpdated { .. } => {
                let old_phase = self.alert_ticks / FLASH_PHASE_TICKS;
                self.alert_ticks = self.alert_ticks.wrapping_add(1);
                let new_phase = self.alert_ticks / FLASH_PHASE_TICKS;
                if new_phase != old_phase {
                    Action::Redraw
                } else {
                    Action::None
                }
            }

            SystemEvent::Tap { x, y } => {
                if layout::left_circle_hit(*x, *y) {
                    // Snooze: manager handles buzz stop + RTC programming
                    // + navigate back.
                    data.alarms.alerting = false;
                    Action::SnoozeAlarm
                } else if layout::right_circle_hit(*x, *y) {
                    // Dismiss.
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

/// Format a days bitmask into a short string like "MON-FRI" or "DAILY".
fn format_days(days: u8, buf: &mut heapless::String<26>) {
    if days == 0x7F {
        let _ = buf.push_str("DAILY");
        return;
    }
    if days == 0x3E {
        let _ = buf.push_str("MON-FRI");
        return;
    }
    if days == 0x41 {
        let _ = buf.push_str("SAT+SUN");
        return;
    }
    // Display order: Mon first. DAY_BIT maps display index to bit.
    let names = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"];
    let mut first = true;
    for i in 0..7 {
        if (days & (1 << DAY_BIT[i])) != 0 {
            if !first { let _ = buf.push_str(" "); }
            let _ = buf.push_str(names[i]);
            first = false;
        }
    }
    if first {
        let _ = buf.push_str("NO DAYS");
    }
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
