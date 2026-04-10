//! System frame - persistent header and footer drawn around every screen.
//!
//! Header (top, inside the rounded-corner zone): large amber clock,
//! date beneath, thin teal battery bar with percentage value, and an
//! amber rule separating the header from the content area.
//!
//! Footer (bottom, inside the rounded-corner zone): amber rule,
//! title-box containing the active screen's name, and a row of
//! diamond markers for the carousel (filled = active).

use core::fmt::Write;

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::Point,
    mono_font::{ascii, MonoTextStyle},
    pixelcolor::Rgb565,
    text::Text,
    Drawable,
};

use super::{big_digits, primitives, theme};
use super::types::{ScreenId, SystemData};

/// Draw the top header (battery, clock, date, divider rule).
pub fn draw_header<D: DrawTarget<Color = Rgb565>>(display: &mut D, data: &SystemData) {
    let w = theme::SCREEN_W as i32;

    // Battery meter along the top edge.
    draw_battery_meter(display, data.battery_percent, w);

    // Large amber clock, centered.
    let time_w = big_digits::TIME_WIDTH;
    let time_x = w / 2 - time_w / 2;
    let time_y = 22;
    big_digits::draw_time(display, time_x, time_y, data.hour, data.minute, theme::AMBER);

    // Date in small dim amber under the clock.
    let date_font = MonoTextStyle::new(&ascii::FONT_6X10, theme::AMBER_DIM);
    let mut date_buf = heapless::String::<12>::new();
    write!(date_buf, "{:04}-{:02}-{:02}", data.year, data.month, data.day).ok();
    let date_w = date_buf.len() as i32 * 6;
    Text::new(&date_buf, Point::new(w / 2 - date_w / 2, time_y + 58), date_font)
        .draw(display).ok();

    // Divider rule just above the content band.
    primitives::section_rule(
        display,
        theme::MARGIN * 2,
        theme::CONTENT_TOP - 2,
        w - theme::MARGIN * 4,
        theme::AMBER_DIM,
    );
}

/// Thin flat teal battery meter along the top of the header, with the
/// percentage rendered on the right.
fn draw_battery_meter<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, percent: Option<u8>, screen_w: i32,
) {
    let pct = percent.unwrap_or(0) as u16;
    let bar_y = 6;
    let bar_h = 4;
    let inset = 60; // stay clear of the top-corner curve
    let bar_w = screen_w - inset * 2 - 40; // leave room for percentage text
    let bar_x = inset;

    // Color shifts: teal normally, amber under 50%, red under 20%.
    let (fill, bg) = if pct > 50 {
        (theme::TEAL, theme::TEAL_DIM)
    } else if pct > 20 {
        (theme::AMBER, theme::AMBER_DIM)
    } else {
        (theme::RED, theme::AMBER_DIM)
    };

    primitives::flat_bar(display, bar_x, bar_y, bar_w, bar_h, pct, 100, fill, bg);

    // Percentage label to the right of the bar.
    let font = MonoTextStyle::new(&ascii::FONT_6X10, theme::AMBER);
    let mut buf = heapless::String::<8>::new();
    let _ = match percent {
        Some(p) => write!(buf, "{:>3}%", p),
        None => write!(buf, "  -%"),
    };
    Text::new(&buf, Point::new(bar_x + bar_w + 6, bar_y + bar_h + 2), font)
        .draw(display).ok();
}

/// Draw the bottom footer (rule, screen title-box, diamond carousel).
pub fn draw_footer<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    active_screen: ScreenId,
    screens: &[ScreenId],
) {
    let w = theme::SCREEN_W as i32;
    let y = theme::FOOTER_Y;

    // Divider rule at the top of the footer band.
    primitives::section_rule(
        display,
        theme::MARGIN * 2,
        y - 8,
        w - theme::MARGIN * 4,
        theme::AMBER_DIM,
    );

    // Screen name rendered inside a title-box, horizontally centered.
    let name = screen_name(active_screen);
    // Estimate title-box width (same math as primitives::title_box).
    let ch_w = 10i32;
    let pad_x = 8i32;
    let box_w = name.len() as i32 * ch_w + pad_x * 2;
    let box_x = w / 2 - box_w / 2;
    primitives::title_box(display, box_x, y, name, theme::TEXT_WHITE, theme::AMBER, &ascii::FONT_10X20);

    // Diamond carousel markers below the title-box.
    let dot_count = screens.len() as i32;
    let dot_spacing = 18;
    let dots_w = dot_count * dot_spacing;
    let dot_start_x = w / 2 - dots_w / 2 + dot_spacing / 2;
    let dot_y = y + 40;

    for (i, &id) in screens.iter().enumerate() {
        let cx = dot_start_x + i as i32 * dot_spacing;
        let active = id == active_screen;
        primitives::diamond(display, cx, dot_y, 4, if active { theme::AMBER_HI } else { theme::AMBER_DIM }, active);
    }
}

fn screen_name(id: ScreenId) -> &'static str {
    match id {
        ScreenId::Status => "STATUS",
        ScreenId::CornerTest => "CORNER",
    }
}
