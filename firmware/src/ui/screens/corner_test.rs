//! Corner measurement test screen.
//!
//! Draws horizontal and vertical lines at the screen edges
//! to determine exactly where the rounded bezel clips content.
//! Temporary - remove once safe area dimensions are known.
//!
//! Change BAND to control how deep the measurement area extends.

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
use crate::ui::theme;
use crate::ui::types::{Action, Screen, SystemData};

/// How many pixels deep the measurement lines extend from each edge.
const BAND: i32 = 98;

pub struct CornerTestScreen;

impl CornerTestScreen {
    pub fn new() -> Self {
        Self
    }
}

impl Screen for CornerTestScreen {
    fn render<D: DrawTarget<Color = Rgb565>>(&self, display: &mut D, _data: &SystemData) {
        display.clear(theme::BG).ok();

        let w = theme::SCREEN_W as i32;
        let h = theme::SCREEN_H as i32;
        let font = MonoTextStyle::new(&ascii::FONT_6X10, theme::YELLOW);

        // -- TOP: horizontal lines every 2px --
        for y in (0..=BAND).step_by(2) {
            let color = if (y / 2) % 2 == 0 { theme::CYAN } else { theme::RED };
            let style = PrimitiveStyle::with_stroke(color, 1);
            Line::new(Point::new(0, y), Point::new(w - 1, y))
                .into_styled(style).draw(display).ok();
        }
        for y in (0..=BAND).step_by(10) {
            let mut buf = heapless::String::<8>::new();
            write!(buf, "{}", y).ok();
            Text::new(&buf, Point::new(w / 2 - 10, y + 9), font).draw(display).ok();
        }

        // -- BOTTOM: horizontal lines every 2px --
        for y in (h - BAND..h).step_by(2) {
            let color = if ((h - y) / 2) % 2 == 0 { theme::CYAN } else { theme::RED };
            let style = PrimitiveStyle::with_stroke(color, 1);
            Line::new(Point::new(0, y), Point::new(w - 1, y))
                .into_styled(style).draw(display).ok();
        }
        for y in (h - BAND..h).step_by(10) {
            let mut buf = heapless::String::<8>::new();
            write!(buf, "{}", y).ok();
            Text::new(&buf, Point::new(w / 2 - 10, y + 9), font).draw(display).ok();
        }

        // -- LEFT: vertical lines every 2px --
        for x in (0..=BAND).step_by(2) {
            let color = if (x / 2) % 2 == 0 { theme::CYAN } else { theme::RED };
            let style = PrimitiveStyle::with_stroke(color, 1);
            Line::new(Point::new(x, 0), Point::new(x, h - 1))
                .into_styled(style).draw(display).ok();
        }

        // -- RIGHT: vertical lines every 2px --
        for x in (w - BAND..w).step_by(2) {
            let color = if ((w - x) / 2) % 2 == 0 { theme::CYAN } else { theme::RED };
            let style = PrimitiveStyle::with_stroke(color, 1);
            Line::new(Point::new(x, 0), Point::new(x, h - 1))
                .into_styled(style).draw(display).ok();
        }

    }

    fn on_event(&mut self, event: &SystemEvent, _data: &SystemData) -> Action {
        match event {
            SystemEvent::PowerButtonLong => Action::Shutdown,
            _ => Action::None,
        }
    }
}
