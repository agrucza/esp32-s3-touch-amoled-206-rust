//! Clock screen - the device home, in the modern smartwatch concept
//! style. Layout:
//!
//! - Top: small grey date headline
//! - Hero: large amber HH:MM in bold sans-serif, floating on black
//! - Below: two bordered dark circles (battery, temperature)
//! - Under circles: small grey labels

use core::fmt::Write;

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{Point, Size},
    pixelcolor::Rgb565,
    primitives::Rectangle,
};

use crate::events::SystemEvent;
use crate::ui::{fonts, primitives, theme};
use crate::ui::types::{Action, Screen, SystemData};

pub struct ClockScreen;

impl ClockScreen {
    pub fn new() -> Self { Self }
}

impl Screen for ClockScreen {
    fn render<D: DrawTarget<Color = Rgb565>>(&self, display: &mut D, data: &SystemData) {
        let w = theme::SCREEN_W as i32;

        // -- Date headline at the top, small grey ---------------------------
        let mut date_buf: heapless::String<24> = heapless::String::new();
        let dow = day_of_week(data.time.year as i32, data.time.month as i32, data.time.day as i32);
        let dow_str = ["MO", "DI", "MI", "DO", "FR", "SA", "SO"][dow as usize];
        let month_str = month_de(data.time.month);
        let _ = write!(date_buf, "{} {:02} {} {}", dow_str, data.time.day, month_str, data.time.year);

        fonts::draw_centered(
            display, &fonts::headline(),
            &date_buf, w / 2, 90,
            theme::TEXT_DIM,
        );

        // -- HERO clock, large amber bold sans-serif on black --------------
        // No pill background - the time is the hero element on its own.
        // The virtual rect below keeps the text at the same vertical
        // position the old pill occupied so the rest of the layout
        // (circles, labels) can stay fixed.
        let time_rect = Rectangle::new(
            Point::new(0, 140),
            Size::new(theme::SCREEN_W as u32, 150),
        );
        let mut time_buf: heapless::String<8> = heapless::String::new();
        let _ = write!(time_buf, "{:02}:{:02}", data.time.hour, data.time.minute);
        fonts::draw_centered_in_rect(
            display, &fonts::hero(),
            &time_buf, time_rect,
            theme::AMBER,
        );

        // -- Two bordered circle "buttons" ----------------------------------
        let radius = 35i32;          // 70 px diameter
        let circle_cy = 350i32;
        let left_cx = 155i32;
        let right_cx = 255i32;

        primitives::circle_button(
            display, left_cx, circle_cy, radius,
            theme::PANEL_BG, Some(theme::AMBER_DIM),
        );
        primitives::circle_button(
            display, right_cx, circle_cy, radius,
            theme::PANEL_BG, Some(theme::AMBER_DIM),
        );

        // Bounding rects of the two circles - used for proper visual
        // centering of the text inside.
        let left_rect = Rectangle::new(
            Point::new(left_cx - radius, circle_cy - radius),
            Size::new((radius * 2) as u32, (radius * 2) as u32),
        );
        let right_rect = Rectangle::new(
            Point::new(right_cx - radius, circle_cy - radius),
            Size::new((radius * 2) as u32, (radius * 2) as u32),
        );

        // -- Battery value inside the left circle ---------------------------
        let bat_pct = data.power.battery_percent.unwrap_or(0);
        let mut bat_buf: heapless::String<8> = heapless::String::new();
        let _ = match data.power.battery_percent {
            Some(p) => write!(bat_buf, "{}%", p),
            None => write!(bat_buf, "-%"),
        };
        let bat_color = match data.power.battery_percent {
            Some(_) => primitives::battery_color(bat_pct),
            None => theme::TEXT_MUTED,
        };
        fonts::draw_centered_in_rect(
            display, &fonts::body(),
            &bat_buf, left_rect,
            bat_color,
        );

        // -- Temperature value inside the right circle ----------------------
        let temp_c = data.motion.temp_raw / 256;
        let mut temp_buf: heapless::String<8> = heapless::String::new();
        let _ = write!(temp_buf, "{}C", temp_c);
        fonts::draw_centered_in_rect(
            display, &fonts::body(),
            &temp_buf, right_rect,
            theme::TEXT_WHITE,
        );

        // -- Small grey labels under the circles ----------------------------
        let label_y = circle_cy + radius + 12;
        fonts::draw_centered(
            display, &fonts::caption(),
            "BATTERY", left_cx, label_y,
            theme::TEXT_DIM,
        );
        fonts::draw_centered(
            display, &fonts::caption(),
            "TEMP", right_cx, label_y,
            theme::TEXT_DIM,
        );
    }

    fn on_event(&mut self, event: &SystemEvent, _data: &SystemData) -> Action {
        match event {
            SystemEvent::PowerButtonLong => Action::Shutdown,
            _ => Action::None,
        }
    }
}

/// German month abbreviations. The font supports Latin-1 so the
/// umlaut in "MÄR" renders correctly.
fn month_de(m: u8) -> &'static str {
    match m {
        1 => "JAN", 2 => "FEB", 3 => "MÄR", 4 => "APR",
        5 => "MAI", 6 => "JUN", 7 => "JUL", 8 => "AUG",
        9 => "SEP", 10 => "OKT", 11 => "NOV", 12 => "DEZ",
        _ => "???",
    }
}

/// Day of week via Zeller's congruence, returning 0=Monday..6=Sunday.
fn day_of_week(year: i32, month: i32, day: i32) -> u32 {
    let (m, y) = if month < 3 { (month + 12, year - 1) } else { (month, year) };
    let k = y % 100;
    let j = y / 100;
    let h = (day + (13 * (m + 1)) / 5 + k + k / 4 + j / 4 + 5 * j) % 7;
    ((h + 5) % 7) as u32
}
