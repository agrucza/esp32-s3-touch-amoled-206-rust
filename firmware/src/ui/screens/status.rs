//! Status screen - three rounded cards: ACCEL, GYRO, ENV (temp + touch).

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
        let mut y = theme::CONTENT_TOP + 6;

        // -- ACCEL card --
        draw_axis_card(display, "ACCEL", m, y, w, 92,
            data.accel_x, data.accel_y, data.accel_z, 4096);
        y += 100;

        // -- GYRO card --
        draw_axis_card(display, "GYRO", m, y, w, 92,
            data.gyro_x, data.gyro_y, data.gyro_z, 2048);
        y += 100;

        // -- ENV card (temp + touch) --
        draw_env_card(display, m, y, w, 92, data);
    }

    fn on_event(&mut self, event: &SystemEvent, _data: &SystemData) -> Action {
        match event {
            SystemEvent::PowerButtonLong => Action::Shutdown,
            _ => Action::None,
        }
    }
}

/// Draw a rounded card containing a title label and three axis bars.
fn draw_axis_card<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    label: &str,
    x: i32, y: i32, w: i32, h: i32,
    vx: i16, vy: i16, vz: i16,
    range: i32,
) {
    // Rounded card background
    primitives::rounded_panel(display, x, y, w, h, theme::CARD_RADIUS, None, Some(theme::AMBER_DIM));

    // Card title in amber
    let title_font = MonoTextStyle::new(&ascii::FONT_6X10, theme::AMBER);
    Text::with_baseline(label, Point::new(x + 12, y + 8), title_font, Baseline::Top)
        .draw(display).ok();

    // Three axis rows
    let lbl_font = MonoTextStyle::new(&ascii::FONT_10X20, theme::AMBER);
    let val_font = MonoTextStyle::new(&ascii::FONT_10X20, theme::TEXT_WHITE);
    let bar_x = x + 120;
    let bar_w = w - 132;

    let axes: [(i16, &str); 3] = [(vx, "X"), (vy, "Y"), (vz, "Z")];
    for (i, (val, label)) in axes.iter().enumerate() {
        let row_y = y + 22 + i as i32 * 20;

        Text::with_baseline(label, Point::new(x + 16, row_y), lbl_font, Baseline::Top)
            .draw(display).ok();

        let mut buf = heapless::String::<8>::new();
        write!(buf, "{:>6}", val).ok();
        Text::with_baseline(&buf, Point::new(x + 36, row_y), val_font, Baseline::Top)
            .draw(display).ok();

        let bar_val = ((*val as i32 + range).clamp(0, range * 2) * 100 / (range * 2)) as u16;
        primitives::flat_bar(display, bar_x, row_y + 6, bar_w, 8, bar_val, 100, theme::AMBER, theme::AMBER_DIM);
    }
}

/// Draw the combined environment card (temperature + touch contact).
fn draw_env_card<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    x: i32, y: i32, w: i32, h: i32,
    data: &SystemData,
) {
    primitives::rounded_panel(display, x, y, w, h, theme::CARD_RADIUS, None, Some(theme::AMBER_DIM));

    let title_font = MonoTextStyle::new(&ascii::FONT_6X10, theme::AMBER);
    Text::with_baseline("ENV", Point::new(x + 12, y + 8), title_font, Baseline::Top)
        .draw(display).ok();

    // TEMP row
    let lbl_font = MonoTextStyle::new(&ascii::FONT_10X20, theme::AMBER);
    let val_font = MonoTextStyle::new(&ascii::FONT_10X20, theme::TEXT_WHITE);

    Text::with_baseline("TEMP", Point::new(x + 16, y + 26), lbl_font, Baseline::Top)
        .draw(display).ok();
    let temp_c = data.temp_raw / 256;
    let mut buf = heapless::String::<8>::new();
    write!(buf, "{}C", temp_c).ok();
    Text::with_baseline(&buf, Point::new(x + 80, y + 26), val_font, Baseline::Top)
        .draw(display).ok();
    let bar_val = (temp_c as u16).clamp(0, 60);
    primitives::flat_bar(display, x + 140, y + 32, w - 152, 8, bar_val, 60, theme::TEAL, theme::TEAL_DIM);

    // TOUCH row
    let active = data.touch_x.is_some();
    Text::with_baseline("TOUCH", Point::new(x + 16, y + 56), lbl_font, Baseline::Top)
        .draw(display).ok();
    match (data.touch_x, data.touch_y) {
        (Some(tx), Some(ty)) => {
            let mut buf = heapless::String::<24>::new();
            write!(buf, "{:>3},{:>3}", tx, ty).ok();
            let color = if active { theme::AMBER_HI } else { theme::TEXT_WHITE };
            let font = MonoTextStyle::new(&ascii::FONT_10X20, color);
            Text::with_baseline(&buf, Point::new(x + 96, y + 56), font, Baseline::Top)
                .draw(display).ok();
        }
        _ => {
            let font = MonoTextStyle::new(&ascii::FONT_10X20, theme::TEXT_DIM);
            Text::with_baseline("NO CONTACT", Point::new(x + 96, y + 56), font, Baseline::Top)
                .draw(display).ok();
        }
    }
}
