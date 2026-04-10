//! Pull-down system panel.
//!
//! Opens on a swipe-down from the header. Covers the top two-thirds of
//! the display with a rounded panel, leaving the active screen
//! visible below for context. Closed by a swipe-up or a tap on the
//! CLOSE pill button.

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::Point,
    mono_font::{ascii, MonoTextStyle},
    pixelcolor::Rgb565,
    primitives::Rectangle,
    text::{Baseline, Text},
    Drawable,
};

use super::primitives;
use super::theme;
use super::types::SystemData;

/// Panel occupies the top two-thirds of the screen.
pub const PANEL_H: i32 = (theme::SCREEN_H as i32) * 2 / 3;

const CLOSE_LABEL: &str = "CLOSE";

/// Rectangle occupied by the CLOSE pill button, computed from the
/// same geometry helper the drawing code uses so hit-tests always
/// match what the user sees.
fn close_rect() -> Rectangle {
    let template = primitives::pill_button_rect(0, 0, CLOSE_LABEL);
    let w = template.size.width as i32;
    let h = template.size.height as i32;
    let x = ((theme::SCREEN_W as i32) - w) / 2;
    let y = PANEL_H - h - 24;
    Rectangle::new(Point::new(x, y), template.size)
}

/// Returns true when (x, y) lands inside the CLOSE pill button.
pub fn hit_close(x: u16, y: u16) -> bool {
    let r = close_rect();
    let px = x as i32;
    let py = y as i32;
    px >= r.top_left.x
        && px < r.top_left.x + r.size.width as i32
        && py >= r.top_left.y
        && py < r.top_left.y + r.size.height as i32
}

/// Render the panel on top of whatever the active screen already drew.
pub fn draw<D: DrawTarget<Color = Rgb565>>(display: &mut D, data: &SystemData) {
    // Rounded panel background. Starts at y=0 - the top rounded corners
    // fall entirely inside the 98px bezel corner zone and are hidden
    // behind the bezel, so visually the panel reads as having square
    // top edges. The bottom corners sit visibly inside the content area.
    let px = theme::MARGIN;
    let py = 0;
    let pw = (theme::SCREEN_W as i32) - theme::MARGIN * 2;
    let ph = PANEL_H;
    primitives::rounded_panel(
        display,
        px, py, pw, ph,
        theme::CARD_RADIUS * 2,
        Some(theme::PANEL_BG),
        Some(theme::AMBER_DIM),
    );

    // SYSTEM pill label at the top-left of the panel.
    let sys_label = "SYSTEM";
    let sys_font = MonoTextStyle::new(&ascii::FONT_10X20, theme::BG);
    let sys_pad_x = 14i32;
    let sys_pad_y = 4i32;
    let sys_w = sys_label.len() as i32 * 10 + sys_pad_x * 2;
    let sys_h = 20 + sys_pad_y * 2;
    let sys_x = px + 24;
    let sys_y = theme::CORNER_R - 4; // hang just below the bezel curve
    primitives::pill_solid(display, sys_x, sys_y, sys_w, sys_h, theme::AMBER);
    Text::with_baseline(
        sys_label,
        Point::new(sys_x + sys_pad_x, sys_y + sys_pad_y),
        sys_font,
        Baseline::Top,
    )
    .draw(display).ok();

    // Data rows: amber labels, white values.
    let lbl_font = MonoTextStyle::new(&ascii::FONT_10X20, theme::AMBER);
    let val_font = MonoTextStyle::new(&ascii::FONT_10X20, theme::TEXT_WHITE);
    let row_x = px + 30;
    let val_x = px + 180;
    let mut row_y = sys_y + sys_h + 24;

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

    // Uptime
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

    // CLOSE pill button (solid amber, black text) centered near the
    // bottom of the panel.
    let r = close_rect();
    primitives::pill_button(
        display,
        r.top_left.x,
        r.top_left.y,
        CLOSE_LABEL,
        theme::BG,
        theme::AMBER,
    );

    // Hint line below the button.
    let hint_font = MonoTextStyle::new(&ascii::FONT_6X10, theme::AMBER_DIM);
    let hint = "swipe up or tap to close";
    let hint_w = hint.len() as i32 * 6;
    let hint_x = ((theme::SCREEN_W as i32) - hint_w) / 2;
    Text::with_baseline(
        hint,
        Point::new(hint_x, r.top_left.y + r.size.height as i32 + 8),
        hint_font,
        Baseline::Top,
    )
    .draw(display).ok();
}
