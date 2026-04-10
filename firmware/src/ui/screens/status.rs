//! Status screen (Mankind Divided styling).
//!
//! Four bracketed content regions: ACCEL, GYRO, TEMP, TOUCH. Amber
//! labels, white values, flat bars.

use core::fmt::Write;

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::Point,
    mono_font::{ascii, MonoTextStyle},
    pixelcolor::Rgb565,
    text::Text,
    Drawable,
};

use crate::events::SystemEvent;
use crate::ui::{primitives, theme};
use crate::ui::types::{Action, Screen, SystemData};

pub struct StatusScreen;

impl StatusScreen {
    pub fn new() -> Self { Self }
}

impl Screen for StatusScreen {
    fn render<D: DrawTarget<Color = Rgb565>>(&self, display: &mut D, data: &SystemData) {
        let m = theme::MARGIN * 2;
        let w = theme::SCREEN_W as i32 - m * 2;
        let y0 = theme::CONTENT_TOP + 6;
        let arm = theme::BRACKET_ARM;

        // -- ACCELEROMETER --
        let y = y0;
        let box_h = 78;
        primitives::bracket_corners(display, m, y, w, box_h, arm, theme::AMBER);
        draw_block_label(display, "ACCEL", m + 6, y - 2);
        draw_axis_rows(display, data.accel_x, data.accel_y, data.accel_z,
            m, y + 10, w, 4096);

        // -- GYROSCOPE --
        let y = y0 + 84;
        primitives::bracket_corners(display, m, y, w, box_h, arm, theme::AMBER);
        draw_block_label(display, "GYRO", m + 6, y - 2);
        draw_axis_rows(display, data.gyro_x, data.gyro_y, data.gyro_z,
            m, y + 10, w, 2048);

        // -- TEMPERATURE --
        let y = y0 + 168;
        let box_h = 42;
        primitives::bracket_corners(display, m, y, w, box_h, arm, theme::AMBER);
        draw_block_label(display, "TEMP", m + 6, y - 2);

        let temp_c = data.temp_raw / 256;
        let mut buf = heapless::String::<8>::new();
        write!(buf, "{}C", temp_c).ok();
        let val_font = MonoTextStyle::new(&ascii::FONT_10X20, theme::TEXT_WHITE);
        Text::new(&buf, Point::new(m + 16, y + 30), val_font).draw(display).ok();

        let bar_val = (temp_c as u16).clamp(0, 60);
        primitives::flat_bar(
            display, m + 100, y + 18, w - 120, 10,
            bar_val, 60, theme::TEAL, theme::TEAL_DIM,
        );

        // -- TOUCH --
        let y = y0 + 222;
        let box_h = 42;
        let active = data.touch_x.is_some();
        let color = if active { theme::AMBER_HI } else { theme::AMBER_DIM };
        primitives::bracket_corners(display, m, y, w, box_h, arm, color);
        draw_block_label_colored(display, "TOUCH", m + 6, y - 2, color);

        match (data.touch_x, data.touch_y) {
            (Some(tx), Some(ty)) => {
                let mut buf = heapless::String::<24>::new();
                write!(buf, "X:{:>3}  Y:{:>3}", tx, ty).ok();
                let font = MonoTextStyle::new(&ascii::FONT_10X20, theme::TEXT_WHITE);
                Text::new(&buf, Point::new(m + 16, y + 30), font).draw(display).ok();
            }
            _ => {
                let font = MonoTextStyle::new(&ascii::FONT_10X20, theme::TEXT_DIM);
                Text::new("NO CONTACT", Point::new(m + 16, y + 30), font).draw(display).ok();
            }
        }
    }

    fn on_event(&mut self, event: &SystemEvent, _data: &SystemData) -> Action {
        match event {
            SystemEvent::PowerButtonLong => Action::Shutdown,
            _ => Action::None,
        }
    }
}

/// Draw an amber block label sitting slightly above the top-left bracket.
fn draw_block_label<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, text: &str, x: i32, y: i32,
) {
    draw_block_label_colored(display, text, x, y, theme::AMBER);
}

fn draw_block_label_colored<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, text: &str, x: i32, y: i32, color: Rgb565,
) {
    let font = MonoTextStyle::new(&ascii::FONT_6X10, color);
    Text::new(text, Point::new(x + 14, y + 10), font).draw(display).ok();
}

/// Draw three X/Y/Z label+value rows with flat bars.
fn draw_axis_rows<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    x: i16, y_val: i16, z: i16,
    m: i32, block_y: i32, w: i32,
    range: i32,
) {
    let lbl_font = MonoTextStyle::new(&ascii::FONT_10X20, theme::AMBER);
    let val_font = MonoTextStyle::new(&ascii::FONT_10X20, theme::TEXT_WHITE);
    let bar_x = m + 108;
    let bar_w = w - 124;

    let axes: [(i16, &str); 3] = [(x, "X"), (y_val, "Y"), (z, "Z")];

    for (i, (val, label)) in axes.iter().enumerate() {
        let row_y = block_y + 4 + i as i32 * 18;

        Text::new(label, Point::new(m + 12, row_y + 14), lbl_font)
            .draw(display).ok();

        let mut buf = heapless::String::<8>::new();
        write!(buf, "{:>6}", val).ok();
        Text::new(&buf, Point::new(m + 32, row_y + 14), val_font)
            .draw(display).ok();

        let bar_val = ((*val as i32 + range).clamp(0, range * 2) * 100 / (range * 2)) as u16;
        primitives::flat_bar(display, bar_x, row_y + 4, bar_w, 8, bar_val, 100, theme::AMBER, theme::AMBER_DIM);
    }
}
