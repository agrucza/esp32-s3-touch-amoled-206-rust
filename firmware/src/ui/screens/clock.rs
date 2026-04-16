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
    geometry::{Point, Size},
    pixelcolor::Rgb565,
    prelude::Primitive,
    primitives::{Circle, Line, PrimitiveStyle, Rectangle},
    Drawable,
};

use crate::events::SystemEvent;
use crate::ui::{fonts, primitives, theme};
use crate::ui::types::{Action, Screen, SystemData};

// -- Layout constants --------------------------------------------------------

/// Y of the date headline (top of glyphs). Clears the bezel arc.
const DATE_Y: i32 = 92;

/// Width of the amber hero pill. Tuned for HH:MM in the 49 px hero
/// font with generous horizontal padding so the pill reads as the
/// dominant element of the face.
const HERO_PILL_W: i32 = 320;
/// Height of the amber hero pill. Pill radius is `h/2` so this is
/// also the corner diameter. Tall enough to wrap the 49 px glyphs
/// with symmetric vertical padding.
const HERO_PILL_H: i32 = 130;
/// Top of the hero pill, measured from the framebuffer top.
const HERO_PILL_Y: i32 = 160;
/// Left edge of the hero pill - horizontally centered on the screen.
const HERO_PILL_X: i32 = (theme::SCREEN_W as i32 - HERO_PILL_W) / 2;

/// Rect that the HH:MM text is centered inside. Matches the pill
/// exactly so the text sits perfectly inside the amber fill.
const HERO_RECT: Rectangle = Rectangle::new(
    Point::new(HERO_PILL_X, HERO_PILL_Y),
    Size::new(HERO_PILL_W as u32, HERO_PILL_H as u32),
);

/// Drawn radius of each bottom circle. Sized so the two circles
/// plus the gap between them span roughly the same width as the
/// hero pill above them - this matches the reference where the
/// circles dominate the lower half of the card rather than sitting
/// as small chips.
const CIRCLE_RADIUS: i32 = 70;
/// Horizontal gap between the two circles (edge-to-edge).
const CIRCLE_GAP: i32 = 24;
/// X center of the left (STOPWATCH / clock glyph) circle.
const LEFT_CIRCLE_CX: i32 =
    theme::SCREEN_W as i32 / 2 - CIRCLE_GAP / 2 - CIRCLE_RADIUS;
/// X center of the right (ALARM / bell glyph) circle.
const RIGHT_CIRCLE_CX: i32 =
    theme::SCREEN_W as i32 / 2 + CIRCLE_GAP / 2 + CIRCLE_RADIUS;
/// Vertical center of the two bottom circles. Positioned so the
/// circle's top edge sits a modest gap below the bottom of the
/// hero pill (HERO_PILL_Y + HERO_PILL_H = 290).
const CIRCLE_CY: i32 = 310 + CIRCLE_RADIUS;
/// Glyph drawing radius - insets the icon inside the circle
/// outline so it doesn't kiss the border. Tuned to ~2/3 of the
/// previous size so the glyph sits as a compact icon inside the
/// big dark disc rather than filling it.
const GLYPH_RADIUS: i32 = CIRCLE_RADIUS - 37;
/// Gap between the bottom of a circle and the top of its caption.
const CIRCLE_LABEL_GAP: i32 = 14;
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
        // Filled amber pill matching the reference watch face, with
        // HH:MM rendered in near-black inside it so the pill itself
        // reads as the hero surface.
        primitives::pill_solid(
            display,
            HERO_PILL_X, HERO_PILL_Y, HERO_PILL_W, HERO_PILL_H,
            theme::AMBER,
        );
        let mut time_buf: heapless::String<8> = heapless::String::new();
        let _ = write!(time_buf, "{:02}:{:02}", data.time.hour, data.time.minute);
        fonts::draw_centered_in_rect(
            display, &fonts::hero(),
            &time_buf, HERO_RECT,
            theme::BG,
        );

        // -- Left circle: clock glyph, STOPWATCH label ----------------------
        primitives::circle_button(
            display,
            LEFT_CIRCLE_CX, CIRCLE_CY, CIRCLE_RADIUS,
            theme::PANEL_BG,
            None,
        );
        draw_hourglass_glyph(
            display,
            LEFT_CIRCLE_CX, CIRCLE_CY, GLYPH_RADIUS,
            theme::TEXT_WHITE,
        );
        fonts::draw_centered(
            display, &fonts::caption(),
            "TIMER",
            LEFT_CIRCLE_CX, CIRCLE_CY + CIRCLE_RADIUS + CIRCLE_LABEL_GAP,
            theme::TEXT_DIM,
        );

        // -- Right circle: bell glyph, ALARM label, notification dot --------
        primitives::circle_button(
            display,
            RIGHT_CIRCLE_CX, CIRCLE_CY, CIRCLE_RADIUS,
            theme::PANEL_BG,
            None,
        );
        draw_bell_glyph(
            display,
            RIGHT_CIRCLE_CX, CIRCLE_CY, GLYPH_RADIUS,
            theme::TEXT_WHITE,
        );
        // Notification dot: sits on the upper-right edge of the
        // circle, roughly on the 45-deg (1:30 o'clock) tangent.
        let dot_offset = (CIRCLE_RADIUS as f32 * 0.707) as i32;
        Circle::with_center(
            Point::new(RIGHT_CIRCLE_CX + dot_offset, CIRCLE_CY - dot_offset),
            (NOTIFY_DOT_RADIUS * 2) as u32,
        )
        .into_styled(PrimitiveStyle::with_fill(theme::GREEN))
        .draw(display)
        .ok();
        fonts::draw_centered(
            display, &fonts::caption(),
            "ALARM",
            RIGHT_CIRCLE_CX, CIRCLE_CY + CIRCLE_RADIUS + CIRCLE_LABEL_GAP,
            theme::TEXT_DIM,
        );
    }

    fn on_event(&mut self, event: &SystemEvent, _data: &SystemData) -> Action {
        match event {
            SystemEvent::PowerButtonLong => Action::Shutdown,
            _ => Action::None,
        }
    }
}

// -- Date helpers ------------------------------------------------------------

/// German month abbreviations. The font supports Latin-1 so the
/// umlaut in "MÄR" renders correctly.
fn month_de(m: u8) -> &'static str {
    match m {
        1  => "JAN", 2  => "FEB", 3  => "MÄR", 4  => "APR",
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

// -- Glyphs ------------------------------------------------------------------
//
// Local copies for now - the panel screen has a `draw_clock_glyph`
// with the same shape. Once Stopwatch, Timer, and Alarm screens
// land (and also need these plus more), extract all glyphs into a
// shared `ui::glyphs` module.

/// Hourglass glyph: two triangular chambers meeting at a point,
/// with flat horizontal caps top and bottom. The "timer is
/// running out" icon.
fn draw_hourglass_glyph<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, cx: i32, cy: i32, radius: i32, color: Rgb565,
) {
    let stroke = PrimitiveStyle::with_stroke(color, 3);

    // Horizontal half-width of the hourglass caps. A touch narrower
    // than the drawing radius so the glyph reads as an hourglass
    // rather than a bowtie.
    let half_w = radius * 4 / 5;
    let top_y    = cy - radius;
    let bottom_y = cy + radius;

    // Top cap.
    Line::new(
        Point::new(cx - half_w, top_y),
        Point::new(cx + half_w, top_y),
    ).into_styled(stroke).draw(display).ok();

    // Left slant down (top cap left -> pinch point).
    Line::new(
        Point::new(cx - half_w, top_y),
        Point::new(cx, cy),
    ).into_styled(stroke).draw(display).ok();

    // Right slant down (top cap right -> pinch point).
    Line::new(
        Point::new(cx + half_w, top_y),
        Point::new(cx, cy),
    ).into_styled(stroke).draw(display).ok();

    // Left slant up (pinch point -> bottom cap left).
    Line::new(
        Point::new(cx, cy),
        Point::new(cx - half_w, bottom_y),
    ).into_styled(stroke).draw(display).ok();

    // Right slant up (pinch point -> bottom cap right).
    Line::new(
        Point::new(cx, cy),
        Point::new(cx + half_w, bottom_y),
    ).into_styled(stroke).draw(display).ok();

    // Bottom cap.
    Line::new(
        Point::new(cx - half_w, bottom_y),
        Point::new(cx + half_w, bottom_y),
    ).into_styled(stroke).draw(display).ok();
}

/// Analog-clock glyph: circle with hour and minute hands, center dot.
#[allow(dead_code)]
fn draw_clock_glyph<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, cx: i32, cy: i32, radius: i32, color: Rgb565,
) {
    let thin  = PrimitiveStyle::with_stroke(color, 2);
    let thick = PrimitiveStyle::with_stroke(color, 3);

    Circle::with_center(Point::new(cx, cy), (radius * 2) as u32)
        .into_styled(thin).draw(display).ok();

    // Minute hand (pointing up)
    Line::new(
        Point::new(cx, cy),
        Point::new(cx, cy - radius * 2 / 3),
    ).into_styled(thick).draw(display).ok();

    // Hour hand (pointing upper-right ~2 o'clock)
    Line::new(
        Point::new(cx, cy),
        Point::new(cx + radius / 3, cy - radius / 3),
    ).into_styled(thick).draw(display).ok();

    // Center cap
    Circle::with_center(Point::new(cx, cy), 4)
        .into_styled(PrimitiveStyle::with_fill(color))
        .draw(display).ok();
}

/// Stylised bell glyph: small handle on top, trapezoidal body
/// flaring to a horizontal base, and a clapper dot below.
fn draw_bell_glyph<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, cx: i32, cy: i32, radius: i32, color: Rgb565,
) {
    let stroke = PrimitiveStyle::with_stroke(color, 2);
    let fill   = PrimitiveStyle::with_fill(color);

    // Handle dot at the very top.
    Circle::with_center(Point::new(cx, cy - radius), 3)
        .into_styled(fill).draw(display).ok();

    // Bell body: narrow at the top, flaring to a wider base.
    let top_half_w  = radius / 3;
    let base_half_w = radius * 3 / 4;
    let top_y  = cy - radius * 2 / 3;
    let base_y = cy + radius / 3;

    // Top cap (narrow horizontal line connecting the two slants).
    Line::new(
        Point::new(cx - top_half_w, top_y),
        Point::new(cx + top_half_w, top_y),
    ).into_styled(stroke).draw(display).ok();

    // Left slant from top cap to base-left.
    Line::new(
        Point::new(cx - top_half_w, top_y),
        Point::new(cx - base_half_w, base_y),
    ).into_styled(stroke).draw(display).ok();

    // Right slant from top cap to base-right.
    Line::new(
        Point::new(cx + top_half_w, top_y),
        Point::new(cx + base_half_w, base_y),
    ).into_styled(stroke).draw(display).ok();

    // Horizontal base.
    Line::new(
        Point::new(cx - base_half_w, base_y),
        Point::new(cx + base_half_w, base_y),
    ).into_styled(stroke).draw(display).ok();

    // Clapper dot just below the base.
    Circle::with_center(Point::new(cx, base_y + 5), 3)
        .into_styled(fill).draw(display).ok();
}
