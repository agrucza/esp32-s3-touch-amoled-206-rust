//! Pull-down system panel (Mankind Divided styling).
//!
//! Opened by a swipe-down in the header. Covers the upper two-thirds of
//! the screen with a near-black overlay and shows quick system info
//! (battery, voltage, uptime) plus a CLOSE text-button. Closed by:
//! - swipe-up anywhere
//! - tap inside the close button hit-box

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{Point, Size},
    mono_font::{ascii, MonoTextStyle},
    pixelcolor::Rgb565,
    prelude::Primitive,
    primitives::{PrimitiveStyle, Rectangle},
    text::{Baseline, Text},
    Drawable,
};

use super::primitives;
use super::theme;
use super::types::SystemData;

/// Panel occupies the top two-thirds of the screen.
pub const PANEL_H: i32 = (theme::SCREEN_H as i32) * 2 / 3;

// -- Close button hit box ----------------------------------------------------
//
// Matches the geometry produced by `primitives::text_button` for the
// label "CLOSE" rendered with FONT_10X20 (10 px per glyph, 14 px padding
// each side -> 78 px wide, 32 px tall).

const CLOSE_LABEL: &str = "CLOSE";
const CLOSE_W: i32 = (CLOSE_LABEL.len() as i32) * 10 + 14 * 2;
const CLOSE_H: i32 = 20 + 6 * 2;
const CLOSE_X: i32 = ((theme::SCREEN_W as i32) - CLOSE_W) / 2;
const CLOSE_Y: i32 = PANEL_H - CLOSE_H - 24;

/// Returns true when (x, y) lands inside the close button's hit box.
pub fn hit_close(x: u16, y: u16) -> bool {
    let x = x as i32;
    let y = y as i32;
    x >= CLOSE_X && x < CLOSE_X + CLOSE_W && y >= CLOSE_Y && y < CLOSE_Y + CLOSE_H
}

/// Render the panel overlay on top of whatever the screen already drew.
/// Fills the top 2/3 of the display with `PANEL_BG` so the active
/// screen remains visible in the bottom third for context.
pub fn draw<D: DrawTarget<Color = Rgb565>>(display: &mut D, data: &SystemData) {
    // Opaque background fill covering the full panel area.
    Rectangle::new(
        Point::new(0, 0),
        Size::new(theme::SCREEN_W as u32, PANEL_H as u32),
    )
    .into_styled(PrimitiveStyle::with_fill(theme::PANEL_BG))
    .draw(display).ok();

    // Content area starts below the rounded-corner zone.
    let content_x = theme::MARGIN * 2;
    let content_y = theme::CORNER_R + 6;
    let content_w = (theme::SCREEN_W as i32) - theme::MARGIN * 4;
    let content_h = PANEL_H - content_y - 8;

    // Bracket corners frame the content area (MD signature panel look).
    primitives::bracket_corners(
        display, content_x, content_y, content_w, content_h,
        theme::BRACKET_ARM, theme::AMBER,
    );

    // :SYSTEM title-box in the top-left of the content area, hanging
    // slightly above the bracket top-left corner.
    let title_rect = primitives::title_box(
        display,
        content_x + 8,
        content_y - 14,
        "SYSTEM",
        theme::TEXT_WHITE,
        theme::AMBER,
        &ascii::FONT_10X20,
    );

    // Data rows: amber labels, white values.
    let lbl_font = MonoTextStyle::new(&ascii::FONT_10X20, theme::AMBER);
    let val_font = MonoTextStyle::new(&ascii::FONT_10X20, theme::TEXT_WHITE);
    let row_x = content_x + 24;
    let val_x = content_x + 170;
    let mut row_y = title_rect.top_left.y + title_rect.size.height as i32 + 20;

    // Battery percentage
    Text::with_baseline("BATTERY", Point::new(row_x, row_y), lbl_font, Baseline::Top)
        .draw(display).ok();
    let mut buf: heapless::String<16> = heapless::String::new();
    if let Some(p) = data.battery_percent {
        let _ = core::fmt::write(&mut buf, format_args!("{:>3} %", p));
    } else {
        let _ = buf.push_str("  - %");
    }
    Text::with_baseline(&buf, Point::new(val_x, row_y), val_font, Baseline::Top)
        .draw(display).ok();
    row_y += 28;

    // Battery voltage
    Text::with_baseline("VOLTAGE", Point::new(row_x, row_y), lbl_font, Baseline::Top)
        .draw(display).ok();
    buf.clear();
    if let Some(mv) = data.battery_voltage_mv {
        let _ = core::fmt::write(
            &mut buf,
            format_args!("{}.{:02} V", mv / 1000, (mv % 1000) / 10),
        );
    } else {
        let _ = buf.push_str("-.-- V");
    }
    Text::with_baseline(&buf, Point::new(val_x, row_y), val_font, Baseline::Top)
        .draw(display).ok();
    row_y += 28;

    // Uptime - tick count * 50 ms tick period
    Text::with_baseline("UPTIME", Point::new(row_x, row_y), lbl_font, Baseline::Top)
        .draw(display).ok();
    let total_secs = (data.tick_count as u64) * 50 / 1000;
    let hh = total_secs / 3600;
    let mm = (total_secs / 60) % 60;
    let ss = total_secs % 60;
    buf.clear();
    let _ = core::fmt::write(&mut buf, format_args!("{:02}:{:02}:{:02}", hh, mm, ss));
    Text::with_baseline(&buf, Point::new(val_x, row_y), val_font, Baseline::Top)
        .draw(display).ok();

    // CLOSE text-button near the bottom of the panel.
    primitives::text_button(
        display, CLOSE_X, CLOSE_Y, CLOSE_LABEL,
        theme::TEXT_WHITE, theme::AMBER,
    );

    // Hint line below the button.
    let hint_font = MonoTextStyle::new(&ascii::FONT_6X10, theme::AMBER_DIM);
    let hint = "swipe up or tap to close";
    let hint_w = hint.len() as i32 * 6;
    let hint_x = ((theme::SCREEN_W as i32) - hint_w) / 2;
    Text::new(hint, Point::new(hint_x, CLOSE_Y + CLOSE_H + 14), hint_font)
        .draw(display).ok();
}
