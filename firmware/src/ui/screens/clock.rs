//! Clock screen - the device home. Full-screen layout with a large
//! amber HH:MM, a German date line, and a small battery indicator
//! (glyph + percentage) beneath it.

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
use crate::ui::{primitives, theme};
use crate::ui::types::{Action, Screen, SystemData};

pub struct ClockScreen;

impl ClockScreen {
    pub fn new() -> Self { Self }
}

impl Screen for ClockScreen {
    fn render<D: DrawTarget<Color = Rgb565>>(&self, display: &mut D, data: &SystemData) {
        let w = theme::SCREEN_W as i32;
        let h = theme::SCREEN_H as i32;

        // Layout: clock + gap + date + gap + battery, vertically centered
        // as a block.
        let style = DigitStyle::LARGE;
        let clock_h = style.h;       // 88
        let date_h: i32 = 20;        // FONT_10X20 height
        let battery_h: i32 = 14;     // battery icon body height
        let gap_clock_date: i32 = 20;
        let gap_date_battery: i32 = 14;
        let block_h = clock_h + gap_clock_date + date_h + gap_date_battery + battery_h;
        let block_top = h / 2 - block_h / 2;

        // Big HH:MM
        let tw = style.time_width();
        let time_x = w / 2 - tw / 2;
        let time_y = block_top;
        big_digits::draw_time(display, time_x, time_y, data.hour, data.minute, theme::AMBER, &style);

        // German date line, e.g. "MO 30 MRZ 2026".
        let mut date_buf: heapless::String<24> = heapless::String::new();
        let dow = day_of_week(data.year as i32, data.month as i32, data.day as i32);
        let dow_str = ["MO", "DI", "MI", "DO", "FR", "SA", "SO"][dow as usize];
        let month_str = month_de(data.month);
        let _ = write!(date_buf, "{} {:02} {} {}", dow_str, data.day, month_str, data.year);

        let date_font = MonoTextStyle::new(&ascii::FONT_10X20, theme::AMBER_DIM);
        let date_w = date_buf.len() as i32 * 10;
        let date_x = w / 2 - date_w / 2;
        let date_y = time_y + clock_h + gap_clock_date;
        Text::with_baseline(&date_buf, Point::new(date_x, date_y), date_font, Baseline::Top)
            .draw(display).ok();

        // Battery indicator: glyph + percentage text, centered as a
        // pair below the date line.
        let battery_y = date_y + date_h + gap_date_battery;
        draw_battery_status(display, w / 2, battery_y, data.battery_percent);
    }

    fn on_event(&mut self, event: &SystemEvent, _data: &SystemData) -> Action {
        match event {
            SystemEvent::PowerButtonLong => Action::Shutdown,
            _ => Action::None,
        }
    }
}

/// Draw a centered "[icon] 87 %" battery status block. The pair is
/// horizontally centered around `cx`; `y` is the top edge of the
/// glyph and text.
fn draw_battery_status<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    cx: i32, y: i32,
    percent: Option<u8>,
) {
    // Icon body is 30 px wide + 3 px nub = 33 px total.
    let icon_w = 33i32;
    let gap = 8i32;
    let text_chars = 5; // "100 %" widest case
    let text_w = text_chars * 10;
    let total_w = icon_w + gap + text_w;
    let x0 = cx - total_w / 2;

    let pct = percent.unwrap_or(0);
    primitives::battery_icon(display, x0, y, pct, theme::AMBER_DIM);

    // Percentage text, same color as the fill for visual linkage.
    let color = match percent {
        Some(_) => primitives::battery_color(pct),
        None => theme::AMBER_DIM,
    };
    let font = MonoTextStyle::new(&ascii::FONT_10X20, color);
    let mut buf: heapless::String<8> = heapless::String::new();
    let _ = match percent {
        Some(p) => write!(buf, "{:>3} %", p),
        None => write!(buf, "  - %"),
    };
    Text::with_baseline(
        &buf,
        Point::new(x0 + icon_w + gap, y - 3),
        font,
        Baseline::Top,
    )
    .draw(display).ok();
}

/// German month abbreviations, no umlauts so the default ASCII font
/// renders them correctly: March = "MRZ", October = "OKT", December = "DEZ".
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
    let (m, y) = if month < 3 { (month + 12, year - 1) } else { (month, year) };
    let k = y % 100;
    let j = y / 100;
    // Zeller's formula with +5J instead of -2J (equivalent mod 7) to
    // keep everything non-negative.
    let h = (day + (13 * (m + 1)) / 5 + k + k / 4 + j / 4 + 5 * j) % 7;
    // Zeller returns 0=Sat..6=Fri; remap to 0=Mon..6=Sun.
    ((h + 5) % 7) as u32
}
