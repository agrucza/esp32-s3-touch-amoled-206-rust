//! Clock screen - the device home face.
//!
//! Layout matches the reference smartwatch concept (the watch-face
//! card in the top-left of the concept sheet):
//!
//! - Top: small grey date headline ("MO 15 APR 2026")
//! - Middle: large filled amber **pill** containing HH:MM in dark
//!   text - the hero element of the face
//! - Bottom: two dark circle icons with small grey captions below.
//!   Left is a clock glyph labelled TIMER, right is a bell glyph
//!   labelled ALARM with a small green notification dot on its
//!   upper-right edge. These are visual only for now; tap routing
//!   to Timer / Alarm gets wired once those screens exist.
//!
//! The clock is a *home-row* app (reachable via the panel launcher
//! and L/R swipes between clock and status), so it has no header
//! bar and no back affordance - tapping the left edge does nothing
//! here. Navigation to other apps happens via the panel pull-down.

use core::fmt::Write;

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::Point,
    pixelcolor::Rgb565,
    prelude::Primitive,
    primitives::{Circle, PrimitiveStyle},
    Drawable,
};

use crate::events::SystemEvent;
use crate::ui::{fonts, glyphs, layout, primitives, theme};
use crate::ui::types::{Action, Screen, ScreenId, SystemData};
use crate::ui::widgets::icon_button;

// -- Layout constants (clock-specific) ---------------------------------------

/// Y of the date headline (top of glyphs). Clears the bezel arc.
const DATE_Y: i32 = 92;

/// Radius of the small green notification dot that overlaps the
/// alarm circle's upper-right edge (reference visual).
const NOTIFY_DOT_RADIUS: i32 = 9;

pub struct ClockScreen;

impl ClockScreen {
    pub fn new() -> Self { Self }
}

impl Screen for ClockScreen {
    fn render<D: DrawTarget<Color = Rgb565>>(&self, display: &mut D, data: &SystemData) {
        let w = theme::SCREEN_W as i32;

        // -- Date headline --------------------------------------------------
        let mut date_buf: heapless::String<24> = heapless::String::new();
        let dow = day_of_week(
            data.time.year as i32,
            data.time.month as i32,
            data.time.day as i32,
        );
        let dow_str = ["MO", "DI", "MI", "DO", "FR", "SA", "SO"][dow as usize];
        let month_str = month_de(data.time.month);
        let _ = write!(
            date_buf,
            "{} {:02} {} {}",
            dow_str, data.time.day, month_str, data.time.year,
        );
        fonts::draw_centered(
            display, &fonts::headline(),
            &date_buf, w / 2, DATE_Y,
            theme::TEXT_DIM,
        );

        // -- HH:MM hero pill -----------------------------------------------
        primitives::pill_solid(
            display,
            layout::HERO_PILL_X, layout::HERO_PILL_Y,
            layout::HERO_PILL_W, layout::HERO_PILL_H,
            theme::AMBER,
        );
        let mut time_buf: heapless::String<8> = heapless::String::new();
        let _ = write!(time_buf, "{:02}:{:02}", data.time.hour, data.time.minute);
        fonts::draw_centered_in_rect(
            display, &fonts::hero(),
            &time_buf, layout::HERO_RECT,
            theme::BG,
        );

        // -- Left circle: hourglass glyph, TIMER label ---------------------
        icon_button(
            display,
            layout::LEFT_CIRCLE_CX, layout::CIRCLE_CY,
            theme::PANEL_BG,
            glyphs::hourglass, theme::TEXT_WHITE,
            "TIMER", theme::TEXT_DIM,
        );

        // -- Right circle: bell glyph, ALARM label, notification dot -------
        icon_button(
            display,
            layout::RIGHT_CIRCLE_CX, layout::CIRCLE_CY,
            theme::PANEL_BG,
            glyphs::bell, theme::TEXT_WHITE,
            "ALARM", theme::TEXT_DIM,
        );
        // Notification dot: sits on the upper-right edge of the
        // circle, roughly on the 45-deg (1:30 o'clock) tangent.
        let dot_offset = (layout::CIRCLE_RADIUS as f32 * 0.707) as i32;
        Circle::with_center(
            Point::new(
                layout::RIGHT_CIRCLE_CX + dot_offset,
                layout::CIRCLE_CY - dot_offset,
            ),
            (NOTIFY_DOT_RADIUS * 2) as u32,
        )
        .into_styled(PrimitiveStyle::with_fill(theme::GREEN))
        .draw(display)
        .ok();
    }

    fn on_event(&mut self, event: &SystemEvent, _data: &mut SystemData) -> Action {
        match event {
            SystemEvent::PowerButtonLong => Action::Shutdown,
            SystemEvent::TimeUpdated { .. } => Action::Redraw,
            SystemEvent::Tap { x, y } if layout::left_circle_hit(*x, *y) => {
                Action::SwitchScreen(ScreenId::Timer)
            }
            _ => Action::None,
        }
    }
}

// -- Date helpers ------------------------------------------------------------

/// German month abbreviations. The font supports Latin-1 so the
/// umlaut in "MAR" renders correctly.
fn month_de(m: u8) -> &'static str {
    match m {
        1  => "JAN", 2  => "FEB", 3  => "MAR", 4  => "APR",
        5  => "MAI", 6  => "JUN", 7  => "JUL", 8  => "AUG",
        9  => "SEP", 10 => "OKT", 11 => "NOV", 12 => "DEZ",
        _  => "???",
    }
}

/// Day of week via Zeller's congruence, returning 0=Monday..6=Sunday.
fn day_of_week(year: i32, month: i32, day: i32) -> u32 {
    let (m, y) = if month < 3 { (month + 12, year - 1) } else { (month, year) };
    let k = y % 100;
    let j = y / 100;
    let h = (day + (13 * (m + 1)) / 5 + k + k / 4 + j / 4 + 5 * j) % 7;
    ((h + 5) % 7) as u32
}
