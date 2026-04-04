//! Cyberpunk HUD status screen.
//!
//! Renders sensor data, touch input, and temperature within the
//! content area (between header and footer frame).

use core::fmt::Write;

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::Point,
    mono_font::{ascii, MonoTextStyle},
    pixelcolor::Rgb565,
    prelude::Primitive,
    primitives::{Line, PrimitiveStyle},
    text::Text,
    Drawable,
};

use crate::events::SystemEvent;
use crate::ui::{theme, primitives};
use crate::ui::types::{Action, Screen, SystemData};

pub struct StatusScreen;

impl StatusScreen {
    pub fn new() -> Self {
        Self
    }
}

impl Screen for StatusScreen {
    fn render<D: DrawTarget<Color = Rgb565>>(&self, display: &mut D, data: &SystemData) {
        let m = theme::MARGIN;
        let w = theme::SCREEN_W as i32;
        let y0 = theme::CONTENT_TOP;
        let cut = theme::CUT;

        // -- ACCELEROMETER --
        let y = y0 + 4;
        let box_h = 90;
        primitives::cut_box(display, m, y, w - m * 2, box_h, theme::CYAN, cut);
        primitives::header_tab(display, m + 2, y - 1, "ACCEL", theme::CYAN, theme::BG);
        draw_axis_bars(display, data.accel_x, data.accel_y, data.accel_z,
            m, y, w, 4096, theme::CYAN, theme::DIM_CYAN);

        // -- GYROSCOPE --
        let y = y0 + 100;
        let box_h = 90;
        primitives::cut_box(display, m, y, w - m * 2, box_h, theme::RED, cut);
        primitives::header_tab(display, m + 2, y - 1, "GYRO", theme::RED, theme::BG);
        draw_axis_bars(display, data.gyro_x, data.gyro_y, data.gyro_z,
            m, y, w, 2048, theme::RED, theme::DARK_RED);

        // -- TEMPERATURE --
        let y = y0 + 196;
        let box_h = 44;
        primitives::cut_box(display, m, y, w - m * 2, box_h, theme::YELLOW, cut);
        primitives::header_tab(display, m + 2, y - 1, "TEMP", theme::YELLOW, theme::BG);

        let temp_c = data.temp_raw / 256;
        let mut buf = heapless::String::<8>::new();
        write!(buf, "{}C", temp_c).ok();
        let font_big = MonoTextStyle::new(&ascii::FONT_10X20, theme::YELLOW);
        Text::new(&buf, Point::new(m + 16, y + 34), font_big).draw(display).ok();

        let bar_val = (temp_c as u16).clamp(0, 60);
        primitives::segmented_bar(
            display, m + 100, y + 18, w - m * 2 - 120, 16,
            bar_val, 60, theme::YELLOW, theme::DIM_CYAN,
        );

        // -- TOUCH --
        let y = y0 + 246;
        let box_h = 54;
        let active = data.touch_x.is_some();
        let border = if active { theme::CYAN } else { theme::DIM_CYAN };
        primitives::cut_box(display, m, y, w - m * 2, box_h, border, cut);
        primitives::header_tab(display, m + 2, y - 1, "TOUCH", border, theme::BG);

        match (data.touch_x, data.touch_y) {
            (Some(tx), Some(ty)) => {
                let mut buf = heapless::String::<24>::new();
                write!(buf, "X:{:>3}  Y:{:>3}", tx, ty).ok();
                let font = MonoTextStyle::new(&ascii::FONT_10X20, theme::CYAN);
                Text::new(&buf, Point::new(m + 16, y + 36), font).draw(display).ok();
            }
            _ => {
                let font = MonoTextStyle::new(&ascii::FONT_10X20, theme::DIM_CYAN);
                Text::new("NO CONTACT", Point::new(m + 16, y + 36), font).draw(display).ok();
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

/// Draw X/Y/Z axis labels and bars inside a block.
fn draw_axis_bars<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    x: i16, y_val: i16, z: i16,
    m: i32, block_y: i32, w: i32,
    range: i32,
    fill: Rgb565, bg: Rgb565,
) {
    let font = MonoTextStyle::new(&ascii::FONT_10X20, fill);
    let bar_x = m + 100;
    let bar_w = w - m * 2 - 120;

    let axes: [(i16, &str); 3] = [(x, "X"), (y_val, "Y"), (z, "Z")];

    for (i, (val, label)) in axes.iter().enumerate() {
        let row_y = block_y + 18 + i as i32 * 24;

        let mut buf = heapless::String::<12>::new();
        write!(buf, "{}:{:>6}", label, val).ok();
        Text::new(&buf, Point::new(m + 12, row_y + 16), font).draw(display).ok();

        let bar_val = ((*val as i32 + range).clamp(0, range * 2) * 100 / (range * 2)) as u16;
        primitives::segmented_bar(display, bar_x, row_y, bar_w, 16, bar_val, 100, fill, bg);
    }
}
