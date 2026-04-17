//! Settings screen - device configuration and diagnostics, organised
//! by hardware subsystem.
//!
//! Uses the **internal state machine** pattern: one [`SettingsScreen`]
//! struct holds a [`SettingsView`] enum that tracks which sub-view is
//! currently shown. Tapping a row in the Index sub-view switches
//! `view` to the corresponding sub-view; tapping the back chevron
//! returns to Index.
//!
//! Visual style matches the widget-layer "All Bookings" reference -
//! all content is built from [`card`] + [`value_body`] + [`header_bar`].

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::Point,
    pixelcolor::Rgb565,
};
use heapless::String;
use core::fmt::Write;

use crate::ui::{fonts, layout, theme};
use crate::ui::types::{
    Action, Screen, SelfTestId, SelfTestResult, SystemData, SystemEvent,
};
use crate::ui::widgets::{
    card, header_bar, value_body, CardStyle, HeaderIcon, Numpad, NumpadAction, MAX_DIGITS,
};

// -- View enum ---------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsView {
    Index,
    Imu,
    Clock,
    TimeEntry,
    DateEntry,
}

// -- Index row metadata ------------------------------------------------------

struct IndexRow {
    label: &'static str,
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

const INDEX_ROWS: &[IndexRow] = &[
    IndexRow {
        label: "CLOCK",
        value_fn: clock_value,
        target: SettingsView::Clock,
    },
    IndexRow {
        label: "6-AXIS IMU",
        value_fn: imu_value,
        target: SettingsView::Imu,
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
        }
    }
}

// -- Index sub-view ----------------------------------------------------------

impl SettingsScreen {
    fn render_index<D: DrawTarget<Color = Rgb565>>(
        &self, display: &mut D, data: &SystemData,
    ) {
        header_bar(
            display,
            layout::header_rect(),
            HeaderIcon::Close,
            "SETTINGS",
            theme::AMBER,
        );

        for (i, row) in INDEX_ROWS.iter().enumerate() {
            let rect = layout::content_card_rect(i);
            card(display, rect, CardStyle::DEFAULT);
            let val = (row.value_fn)(data);
            value_body(display, rect, row.label, val.as_str(), theme::TEXT_WHITE);
        }
    }

    fn index_event(&mut self, event: &SystemEvent) -> Action {
        match event {
            SystemEvent::Tap { x, y } if layout::header_icon_hit(*x, *y) => {
                Action::Back
            }
            SystemEvent::Tap { x, y } => {
                for (i, row) in INDEX_ROWS.iter().enumerate() {
                    if layout::content_card_rect(i)
                        .contains(Point::new(*x as i32, *y as i32))
                    {
                        self.view = row.target;
                        return Action::Redraw;
                    }
                }
                Action::None
            }
            _ => Action::None,
        }
    }
}

// -- Clock sub-view (time + date cards) --------------------------------------

impl SettingsScreen {
    fn render_clock<D: DrawTarget<Color = Rgb565>>(
        &self, display: &mut D, data: &SystemData,
    ) {
        header_bar(
            display,
            layout::header_rect(),
            HeaderIcon::Back,
            "CLOCK",
            theme::AMBER,
        );

        // Time card.
        let rect = layout::content_card_rect(0);
        card(display, rect, CardStyle::DEFAULT);
        let mut time_buf: String<12> = String::new();
        let _ = write!(time_buf, "{:02}:{:02}:{:02}",
            data.time.hour, data.time.minute, data.time.second);
        value_body(display, rect, "TIME", time_buf.as_str(), theme::TEXT_WHITE);

        // Date card.
        let rect = layout::content_card_rect(1);
        card(display, rect, CardStyle::DEFAULT);
        let mut date_buf: String<12> = String::new();
        let _ = write!(date_buf, "{:02}.{:02}.{:04}",
            data.time.day, data.time.month, data.time.year);
        value_body(display, rect, "DATE", date_buf.as_str(), theme::TEXT_WHITE);
    }

    fn clock_event(&mut self, event: &SystemEvent, data: &SystemData) -> Action {
        match event {
            // Keep the display fresh.
            SystemEvent::TimeUpdated { .. } => Action::Redraw,

            SystemEvent::Tap { x, y } if layout::header_icon_hit(*x, *y) => {
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
                    self.numpad.clear();
                    let t = &data.time;
                    push_two_digits(&mut self.numpad.digits, t.hour);
                    push_two_digits(&mut self.numpad.digits, t.minute);
                    push_two_digits(&mut self.numpad.digits, t.second);
                    self.view = SettingsView::TimeEntry;
                    Action::Redraw
                } else if layout::content_card_rect(1)
                    .contains(Point::new(*x as i32, *y as i32))
                {
                    // Open date numpad, pre-fill with current date.
                    self.numpad = Numpad::new(8);
                    self.numpad.clear();
                    let t = &data.time;
                    push_two_digits(&mut self.numpad.digits, t.day);
                    push_two_digits(&mut self.numpad.digits, t.month);
                    push_four_digits(&mut self.numpad.digits, t.year);
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
        &self, display: &mut D, _data: &SystemData,
    ) {
        header_bar(
            display,
            layout::header_rect(),
            HeaderIcon::Back,
            "SET TIME",
            theme::AMBER,
        );

        // HH:MM:SS label from digits.
        let p = pad_digits(&self.numpad.digits, 6);
        let mut buf: String<12> = String::new();
        let _ = write!(buf, "{}{}{}{}{}{}{}{}",
            p[0], p[1], ':', p[2], p[3], ':', p[4], p[5]);
        fonts::draw_centered(
            display, &fonts::value(),
            &buf,
            theme::SCREEN_W as i32 / 2, NUMPAD_TIME_Y,
            theme::AMBER,
        );

        self.numpad.render(display);
    }

    fn time_entry_event(&mut self, event: &SystemEvent, data: &mut SystemData) -> Action {
        match event {
            SystemEvent::Tap { x, y } if layout::header_icon_hit(*x, *y) => {
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
        &self, display: &mut D, _data: &SystemData,
    ) {
        header_bar(
            display,
            layout::header_rect(),
            HeaderIcon::Back,
            "SET DATE",
            theme::AMBER,
        );

        // DD.MM.YYYY label from digits.
        let p = pad_digits(&self.numpad.digits, 8);
        let mut buf: String<12> = String::new();
        let _ = write!(buf, "{}{}.{}{}.{}{}{}{}",
            p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7]);
        fonts::draw_centered(
            display, &fonts::value(),
            &buf,
            theme::SCREEN_W as i32 / 2, NUMPAD_TIME_Y,
            theme::AMBER,
        );

        self.numpad.render(display);
    }

    fn date_entry_event(&mut self, event: &SystemEvent, data: &mut SystemData) -> Action {
        match event {
            SystemEvent::Tap { x, y } if layout::header_icon_hit(*x, *y) => {
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
        header_bar(
            display,
            layout::header_rect(),
            HeaderIcon::Back,
            "6-AXIS IMU",
            theme::AMBER,
        );

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
            SystemEvent::Tap { x, y } if layout::header_icon_hit(*x, *y) => {
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

/// Push a two-digit value (0-99) as individual digits.
fn push_two_digits(digits: &mut heapless::Vec<u8, MAX_DIGITS>, val: u8) {
    let _ = digits.push(val / 10);
    let _ = digits.push(val % 10);
}

/// Push a four-digit value (0-9999) as individual digits.
fn push_four_digits(digits: &mut heapless::Vec<u8, MAX_DIGITS>, val: u16) {
    let _ = digits.push((val / 1000) as u8);
    let _ = digits.push(((val / 100) % 10) as u8);
    let _ = digits.push(((val / 10) % 10) as u8);
    let _ = digits.push((val % 10) as u8);
}

fn format_result(
    result: &SelfTestResult,
    unit: &'static str,
) -> (String<32>, Rgb565, Option<Rgb565>) {
    let mut buf: String<32> = String::new();
    match result {
        SelfTestResult::NotRun => {
            let _ = buf.push_str("--");
            (buf, theme::TEXT_MUTED, None)
        }
        SelfTestResult::Running => {
            let _ = buf.push_str("RUNNING");
            (buf, theme::TEXT_DIM, Some(theme::AMBER))
        }
        SelfTestResult::PassAxes3(v) => {
            let _ = write!(&mut buf, "{} {} {} {}", v[0], v[1], v[2], unit);
            (buf, theme::TEXT_WHITE, Some(theme::GREEN))
        }
        SelfTestResult::FailAxes3(v) => {
            let _ = write!(&mut buf, "{} {} {} {}", v[0], v[1], v[2], unit);
            (buf, theme::RED, Some(theme::RED))
        }
        SelfTestResult::Error(_) => {
            let _ = buf.push_str("ERROR");
            (buf, theme::RED, Some(theme::RED))
        }
    }
}

fn dimmed(mut style: CardStyle) -> CardStyle {
    style.bg = theme::TEXT_MUTED;
    style
}
