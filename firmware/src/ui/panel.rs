//! Pull-down system panel.
//!
//! Opens on a swipe-down from the header. Covers the top two-thirds
//! of the display with a rounded panel, leaving the active screen
//! visible below for context. Closed by a swipe-up or a tap on the
//! CLOSE pill button.

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{Point, Size},
    pixelcolor::Rgb565,
    primitives::Rectangle,
};

use crate::ui::{fonts, primitives, theme};
use crate::ui::types::SystemData;

/// Panel occupies the top two-thirds of the screen.
pub const PANEL_H: i32 = (theme::SCREEN_H as i32) * 2 / 3;

const CLOSE_LABEL: &str = "CLOSE";
const CLOSE_PAD_X: i32 = 18;
const CLOSE_PAD_Y: i32 = 8;
/// Approximate height of the Value font (helvB24, ~24 px tall).
/// Used to size the CLOSE pill so its content sits comfortably.
const VALUE_FONT_H: i32 = 24;

/// Rectangle occupied by the CLOSE pill button. Computed once from
/// the actual u8g2 text width so the draw code and the hit-test
/// code can never disagree on geometry.
fn close_rect() -> Rectangle {
    let text_w = fonts::measure_width(&fonts::value(), CLOSE_LABEL);
    let w = text_w + CLOSE_PAD_X * 2;
    let h = VALUE_FONT_H + CLOSE_PAD_Y * 2;
    let x = ((theme::SCREEN_W as i32) - w) / 2;
    let y = PANEL_H - h - 24;
    Rectangle::new(Point::new(x, y), Size::new(w as u32, h as u32))
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
    // -- Rounded panel background --------------------------------------------
    // Starts at y=0 - the top rounded corners fall inside the bezel
    // corner zone and are hidden, so visually the panel reads as
    // having square top edges.
    let px = theme::MARGIN;
    let py = 0;
    let pw = (theme::SCREEN_W as i32) - theme::MARGIN * 2;
    let ph = PANEL_H;
    primitives::rounded_panel(
        display, px, py, pw, ph,
        theme::CARD_RADIUS * 2,
        Some(theme::PANEL_BG),
        Some(theme::AMBER_DIM),
    );

    // -- SYSTEM amber pill label in the top-left ----------------------------
    let sys_label = "SYSTEM";
    let sys_text_w = fonts::measure_width(&fonts::value(), sys_label);
    let sys_pad_x = 14i32;
    let sys_pad_y = 6i32;
    let sys_w = sys_text_w + sys_pad_x * 2;
    let sys_h = VALUE_FONT_H + sys_pad_y * 2;
    let sys_x = px + 24;
    let sys_y = theme::CORNER_R - 4;
    let sys_rect = Rectangle::new(
        Point::new(sys_x, sys_y),
        Size::new(sys_w as u32, sys_h as u32),
    );
    primitives::pill_solid(display, sys_x, sys_y, sys_w, sys_h, theme::AMBER);
    fonts::draw_centered_in_rect(
        display, &fonts::value(),
        sys_label, sys_rect,
        theme::BG,
    );

    // -- Data rows: amber labels, white values ------------------------------
    let row_x = px + 30;
    let val_x = px + 180;
    let mut row_y = sys_y + sys_h + 24;
    let row_step = 30i32;

    // Battery percentage
    fonts::draw_at(display, &fonts::body(), "BATTERY", row_x, row_y, theme::AMBER);
    let mut buf: heapless::String<16> = heapless::String::new();
    if let Some(p) = data.battery_percent {
        let _ = core::fmt::write(&mut buf, format_args!("{:>3} %", p));
    } else {
        let _ = buf.push_str("  - %");
    }
    fonts::draw_at(display, &fonts::body(), &buf, val_x, row_y, theme::TEXT_WHITE);
    row_y += row_step;

    // Battery voltage
    fonts::draw_at(display, &fonts::body(), "VOLTAGE", row_x, row_y, theme::AMBER);
    buf.clear();
    if let Some(mv) = data.battery_voltage_mv {
        let _ = core::fmt::write(
            &mut buf,
            format_args!("{}.{:02} V", mv / 1000, (mv % 1000) / 10),
        );
    } else {
        let _ = buf.push_str("-.-- V");
    }
    fonts::draw_at(display, &fonts::body(), &buf, val_x, row_y, theme::TEXT_WHITE);
    row_y += row_step;

    // Uptime - tick count * 50 ms tick period
    fonts::draw_at(display, &fonts::body(), "UPTIME", row_x, row_y, theme::AMBER);
    let total_secs = (data.tick_count as u64) * 50 / 1000;
    let hh = total_secs / 3600;
    let mm = (total_secs / 60) % 60;
    let ss = total_secs % 60;
    buf.clear();
    let _ = core::fmt::write(&mut buf, format_args!("{:02}:{:02}:{:02}", hh, mm, ss));
    fonts::draw_at(display, &fonts::body(), &buf, val_x, row_y, theme::TEXT_WHITE);

    // -- CLOSE pill button at the bottom of the panel ----------------------
    let r = close_rect();
    primitives::pill_solid(
        display,
        r.top_left.x, r.top_left.y,
        r.size.width as i32, r.size.height as i32,
        theme::AMBER,
    );
    fonts::draw_centered_in_rect(
        display, &fonts::value(),
        CLOSE_LABEL, r,
        theme::BG,
    );

    // -- Hint line under the close button ----------------------------------
    let hint = "swipe up or tap to close";
    let hint_y = r.top_left.y + r.size.height as i32 + 8;
    fonts::draw_centered(
        display, &fonts::caption(),
        hint,
        (theme::SCREEN_W as i32) / 2,
        hint_y,
        theme::AMBER_DIM,
    );
}
