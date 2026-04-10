//! Pull-down system panel.
//!
//! Opened by a swipe-down gesture in the header. Covers the upper 2/3
//! of the screen as an opaque overlay. Shows quick system info
//! (battery, voltage, uptime) and a close button. Closed by:
//! - swipe-up anywhere
//! - tap inside the close button hit-box

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{Point, Size},
    mono_font::{ascii, MonoTextStyle},
    pixelcolor::Rgb565,
    prelude::Primitive,
    primitives::{PrimitiveStyle, Rectangle},
    text::Text,
    Drawable,
};

use super::primitives::cut_box;
use super::theme;
use super::types::SystemData;

/// Panel occupies the top two-thirds of the screen.
pub const PANEL_H: i32 = (theme::SCREEN_H as i32) * 2 / 3;

// -- Close button geometry (screen coords) -----------------------------------

const CLOSE_W: i32 = 120;
const CLOSE_H: i32 = 32;
const CLOSE_X: i32 = ((theme::SCREEN_W as i32) - CLOSE_W) / 2;
const CLOSE_Y: i32 = PANEL_H - CLOSE_H - 24;

/// Returns true when (x, y) falls inside the close button's hit box.
pub fn hit_close(x: u16, y: u16) -> bool {
    let x = x as i32;
    let y = y as i32;
    x >= CLOSE_X && x < CLOSE_X + CLOSE_W && y >= CLOSE_Y && y < CLOSE_Y + CLOSE_H
}

/// Render the panel overlay on top of whatever the screen drew. Fills the
/// top 2/3 of the display with PANEL_BG so the active screen remains
/// visible in the bottom third.
pub fn draw<D: DrawTarget<Color = Rgb565>>(display: &mut D, data: &SystemData) {
    // Opaque background fill for the full panel area.
    Rectangle::new(
        Point::new(0, 0),
        Size::new(theme::SCREEN_W as u32, PANEL_H as u32),
    )
    .into_styled(PrimitiveStyle::with_fill(theme::PANEL_BG))
    .draw(display).ok();

    // Title (placed below the rounded top-corner zone so it is fully visible)
    let title_font = MonoTextStyle::new(&ascii::FONT_10X20, theme::CYAN);
    let title = "SYSTEM";
    let title_w = title.len() as i32 * 10;
    let title_x = ((theme::SCREEN_W as i32) - title_w) / 2;
    let title_y = theme::CORNER_R + 20;
    Text::new(title, Point::new(title_x, title_y), title_font)
        .draw(display).ok();

    // Data rows
    let row_font_lbl = MonoTextStyle::new(&ascii::FONT_10X20, theme::DIM_CYAN);
    let row_font_val = MonoTextStyle::new(&ascii::FONT_10X20, theme::YELLOW);
    let row_x = theme::MARGIN + 24;
    let val_x = theme::MARGIN + 170;
    let mut row_y = title_y + 42;

    // Battery percent
    Text::new("BATTERY", Point::new(row_x, row_y), row_font_lbl)
        .draw(display).ok();
    let mut buf: heapless::String<16> = heapless::String::new();
    if let Some(p) = data.battery_percent {
        let _ = core::fmt::write(&mut buf, format_args!("{:>3} %", p));
    } else {
        let _ = buf.push_str("  - %");
    }
    Text::new(&buf, Point::new(val_x, row_y), row_font_val)
        .draw(display).ok();
    row_y += 28;

    // Battery voltage
    Text::new("VOLTAGE", Point::new(row_x, row_y), row_font_lbl)
        .draw(display).ok();
    buf.clear();
    if let Some(mv) = data.battery_voltage_mv {
        let _ = core::fmt::write(&mut buf, format_args!("{}.{:02} V", mv / 1000, (mv % 1000) / 10));
    } else {
        let _ = buf.push_str("-.-- V");
    }
    Text::new(&buf, Point::new(val_x, row_y), row_font_val)
        .draw(display).ok();
    row_y += 28;

    // Uptime (tick_count ticks every 50ms)
    Text::new("UPTIME", Point::new(row_x, row_y), row_font_lbl)
        .draw(display).ok();
    let total_secs = (data.tick_count as u64) * 50 / 1000;
    let hh = total_secs / 3600;
    let mm = (total_secs / 60) % 60;
    let ss = total_secs % 60;
    buf.clear();
    let _ = core::fmt::write(&mut buf, format_args!("{:02}:{:02}:{:02}", hh, mm, ss));
    Text::new(&buf, Point::new(val_x, row_y), row_font_val)
        .draw(display).ok();

    // Close button (filled rect + label)
    Rectangle::new(Point::new(CLOSE_X, CLOSE_Y), Size::new(CLOSE_W as u32, CLOSE_H as u32))
        .into_styled(PrimitiveStyle::with_fill(theme::DARK_RED))
        .draw(display).ok();
    cut_box(display, CLOSE_X, CLOSE_Y, CLOSE_W, CLOSE_H, theme::RED, 4);
    let close_font = MonoTextStyle::new(&ascii::FONT_10X20, theme::RED);
    let label = "CLOSE";
    let label_w = label.len() as i32 * 10;
    let lx = CLOSE_X + (CLOSE_W - label_w) / 2;
    let ly = CLOSE_Y + CLOSE_H - 10;
    Text::new(label, Point::new(lx, ly), close_font)
        .draw(display).ok();

    // Hint text below the button
    let hint_font = MonoTextStyle::new(&ascii::FONT_6X10, theme::DIM_CYAN);
    let hint = "swipe up or tap to close";
    let hint_w = hint.len() as i32 * 6;
    let hint_x = ((theme::SCREEN_W as i32) - hint_w) / 2;
    Text::new(hint, Point::new(hint_x, CLOSE_Y + CLOSE_H + 14), hint_font)
        .draw(display).ok();
}
