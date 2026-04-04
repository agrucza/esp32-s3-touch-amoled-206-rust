//! System frame - persistent header and footer drawn around all screens.
//!
//! Header: large time display, date
//! Footer: screen name, carousel dots
//!
//! Content is horizontally centered to stay within the curved bezel edges.

use core::fmt::Write;

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{Point, Size},
    mono_font::{ascii, MonoTextStyle},
    pixelcolor::Rgb565,
    prelude::Primitive,
    primitives::{Circle, PrimitiveStyle, Rectangle},
    text::Text,
    Drawable,
};

use super::{theme, big_digits};
use super::types::{ScreenId, SystemData};

/// Draw the top status bar (battery bar, time, date).
pub fn draw_header<D: DrawTarget<Color = Rgb565>>(display: &mut D, data: &SystemData) {
    let w = theme::SCREEN_W as i32;

    // Battery bar across the top edge
    draw_battery_bar(display, data.battery_percent, w);

    // Large time - centered in the rounded zone
    let time_w = big_digits::TIME_WIDTH;
    let time_x = w / 2 - time_w / 2;
    let time_y = 28;
    big_digits::draw_time(display, time_x, time_y, data.hour, data.minute, theme::CYAN);

    // Date - small, centered below time
    let date_font = MonoTextStyle::new(&ascii::FONT_10X20, theme::DIM_CYAN);
    let mut date_buf = heapless::String::<12>::new();
    write!(date_buf, "{:04}-{:02}-{:02}", data.year, data.month, data.day).ok();
    let date_w = date_buf.len() as i32 * 10;
    Text::new(&date_buf, Point::new(w / 2 - date_w / 2, time_y + 60), date_font)
        .draw(display).ok();

}

/// Draw a thin dotted battery bar along the very top of the screen.
///
/// Color shifts from cyan (>50%) to yellow (20-50%) to red (<20%).
fn draw_battery_bar<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, percent: Option<u8>, screen_w: i32,
) {
    let pct = percent.unwrap_or(0) as i32;
    let bar_y = 6;
    let bar_h = 3;
    let bar_margin = 40; // inset from edges (rounded corners)
    let bar_w = screen_w - bar_margin * 2;
    let filled_w = (pct * bar_w) / 100;

    let color = if pct > 50 {
        theme::CYAN
    } else if pct > 20 {
        theme::YELLOW
    } else {
        theme::RED
    };

    // Draw dotted filled portion
    let seg_w = 3;
    let gap = 2;
    let mut x = bar_margin;
    while x < bar_margin + filled_w {
        let w = seg_w.min(bar_margin + filled_w - x);
        Rectangle::new(
            Point::new(x, bar_y),
            Size::new(w as u32, bar_h as u32),
        )
        .into_styled(PrimitiveStyle::with_fill(color))
        .draw(display).ok();
        x += seg_w + gap;
    }

    // Draw dotted empty portion
    while x < bar_margin + bar_w {
        let w = seg_w.min(bar_margin + bar_w - x);
        Rectangle::new(
            Point::new(x, bar_y),
            Size::new(w as u32, bar_h as u32),
        )
        .into_styled(PrimitiveStyle::with_fill(theme::DIM_CYAN))
        .draw(display).ok();
        x += seg_w + gap;
    }
}

/// Draw the bottom navigation bar (screen name, carousel dots).
pub fn draw_footer<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    active_screen: ScreenId,
    screens: &[ScreenId],
) {
    let w = theme::SCREEN_W as i32;
    let y = theme::FOOTER_Y;

    // Screen name - centered
    let name = screen_name(active_screen);
    let name_font = MonoTextStyle::new(&ascii::FONT_10X20, theme::YELLOW);
    let name_w = name.len() as i32 * 10;
    Text::new(name, Point::new(w / 2 - name_w / 2, y + 10), name_font)
        .draw(display).ok();

    // Carousel dots - centered below name
    let dot_count = screens.len() as i32;
    let dot_spacing = 16;
    let dots_w = dot_count * dot_spacing;
    let dot_start_x = w / 2 - dots_w / 2;

    for (i, &id) in screens.iter().enumerate() {
        let cx = dot_start_x + i as i32 * dot_spacing + 4;
        let cy = y + 24;
        if id == active_screen {
            Circle::new(Point::new(cx, cy), 8)
                .into_styled(PrimitiveStyle::with_fill(theme::CYAN))
                .draw(display).ok();
        } else {
            Circle::new(Point::new(cx, cy), 8)
                .into_styled(PrimitiveStyle::with_stroke(theme::DIM_CYAN, 1))
                .draw(display).ok();
        }
    }
}

fn screen_name(id: ScreenId) -> &'static str {
    match id {
        ScreenId::Status => "STATUS",
        ScreenId::CornerTest => "CORNER TEST",
    }
}
