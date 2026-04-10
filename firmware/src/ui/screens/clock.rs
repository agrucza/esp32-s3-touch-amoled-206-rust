//! Clock screen - large amber HH:MM and a German date line.
//!
//! This is the default startup screen and the "home" of the device.
//! The frame renders a minimal header (battery only) when this screen
//! is active, so the big clock has the full content area to itself.

use core::fmt::Write;

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::Point,
    mono_font::{ascii, MonoTextStyle},
    pixelcolor::Rgb565,
    text::{Baseline, Text},
    Drawable,
};

use crate::events::SystemEvent;
use crate::ui::big_digits::{self, DigitStyle};
use crate::ui::theme;
use crate::ui::types::{Action, Screen, SystemData};

pub struct ClockScreen;

impl ClockScreen {
    pub fn new() -> Self { Self }
}

impl Screen for ClockScreen {
    fn render<D: DrawTarget<Color = Rgb565>>(&self, display: &mut D, data: &SystemData) {
        let w = theme::SCREEN_W as i32;

        // Big HH:MM centered in the content area, biased slightly up so
        // the date line has breathing room below it.
        let style = DigitStyle::LARGE;
        let tw = style.time_width();
        let time_x = w / 2 - tw / 2;
        let content_mid = theme::CONTENT_TOP + theme::CONTENT_H / 2;
        let time_y = content_mid - style.h / 2 - 16;
        big_digits::draw_time(display, time_x, time_y, data.hour, data.minute, theme::AMBER, &style);

        // German date line: "MO 30 MRZ 2026"
        let mut date_buf: heapless::String<24> = heapless::String::new();
        let dow = day_of_week(data.year as i32, data.month as i32, data.day as i32);
        let dow_str = ["MO", "DI", "MI", "DO", "FR", "SA", "SO"][dow as usize];
        let month_str = month_de(data.month);
        let _ = write!(date_buf, "{} {:02} {} {}", dow_str, data.day, month_str, data.year);

        let date_font = MonoTextStyle::new(&ascii::FONT_10X20, theme::AMBER_DIM);
        let date_w = date_buf.len() as i32 * 10;
        let date_x = w / 2 - date_w / 2;
        let date_y = time_y + style.h + 20;
        Text::with_baseline(&date_buf, Point::new(date_x, date_y), date_font, Baseline::Top)
            .draw(display).ok();
    }

    fn on_event(&mut self, event: &SystemEvent, _data: &SystemData) -> Action {
        match event {
            SystemEvent::PowerButtonLong => Action::Shutdown,
            _ => Action::None,
        }
    }
}

/// German month short names. No umlauts so the default ASCII font
/// renders them: March is "MRZ", October "OKT", December "DEZ".
fn month_de(m: u8) -> &'static str {
    match m {
        1 => "JAN", 2 => "FEB", 3 => "MRZ", 4 => "APR",
        5 => "MAI", 6 => "JUN", 7 => "JUL", 8 => "AUG",
        9 => "SEP", 10 => "OKT", 11 => "NOV", 12 => "DEZ",
        _ => "???",
    }
}

/// Day of week via Zeller's congruence, returning 0=Monday..6=Sunday.
fn day_of_week(year: i32, month: i32, day: i32) -> u32 {
    // Shift Jan/Feb to the previous year so Zeller's formula works.
    let (m, y) = if month < 3 { (month + 12, year - 1) } else { (month, year) };
    let k = y % 100;
    let j = y / 100;
    // Zeller's: h = (q + 13(m+1)/5 + K + K/4 + J/4 - 2J) mod 7
    // Using +5J instead of -2J (equivalent mod 7) to keep everything positive.
    let h = (day + (13 * (m + 1)) / 5 + k + k / 4 + j / 4 + 5 * j) % 7;
    // Zeller returns: 0=Sat, 1=Sun, 2=Mon, ..., 6=Fri.
    // Convert to 0=Mon, 1=Tue, ..., 6=Sun.
    ((h + 5) % 7) as u32
}
