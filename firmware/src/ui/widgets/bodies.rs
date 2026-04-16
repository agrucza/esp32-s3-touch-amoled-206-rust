//! Content body helpers - layouts that draw into a `Rectangle`.
//!
//! Body helpers are the "what goes inside" counterpart to
//! [`containers`]. They take a rect and draw a specific content
//! layout into it. They don't own state, don't draw backgrounds,
//! don't care what (if anything) is under them. Use them composed
//! with a [`card`] for the standard look, or standalone on a bare
//! rect for inline values without a panel.
//!
//! Every body helper follows the same signature shape:
//! `(display, rect, ...content, color)`. Screens compute the rect
//! and call the helper.
//!
//! [`containers`]: super::containers
//! [`card`]: super::containers::card

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{Point, Size},
    pixelcolor::Rgb565,
    primitives::Rectangle,
};

use crate::ui::{fonts, primitives, theme};

// -- Layout constants --------------------------------------------------------
//
// Tuned against the "All Bookings" reference visual: small grey
// label sitting near the top of the card, large bold value centered
// below it in the lower half. Offsets are from the top of the rect.

/// Vertical position of the label text (top of glyphs), measured
/// from the top of the card rect. Picks a pleasing ~25% inset on
/// an 80-90 px tall card.
const LABEL_TOP_OFFSET: i32 = 20;

/// Vertical position of the value text (top of glyphs), measured
/// from the top of the card rect. Leaves a ~18 px gap between the
/// label baseline and the value top at the current font sizes.
const VALUE_TOP_OFFSET: i32 = 44;

// -- value_body --------------------------------------------------------------

/// Render a "small grey label over large value" layout into `rect`.
///
/// * `label` is drawn in the [`fonts::body`] style in [`theme::TEXT_DIM`],
///   horizontally centered near the top of the rect.
/// * `value` is drawn in the [`fonts::value`] style (bold) in
///   `value_color`, horizontally centered below the label.
///
/// This is the layout used by every card in the "All Bookings"
/// reference - date label on top, identifier value below. Diagnostic
/// results ("ACCEL X" / "721 mg"), status readings ("BATTERY" /
/// "87%"), and settings summaries ("WIFI" / "Connected") all fit it.
///
/// The `value_color` parameter lets the screen tint the value for
/// semantic meaning (white neutral, green pass, red fail, amber
/// warning). Labels always use `TEXT_DIM` so the value pops.
pub fn value_body<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    rect: Rectangle,
    label: &str,
    value: &str,
    value_color: Rgb565,
) {
    let cx = rect.top_left.x + rect.size.width as i32 / 2;
    let top = rect.top_left.y;

    fonts::draw_centered(
        display, &fonts::body(),
        label, cx, top + LABEL_TOP_OFFSET,
        theme::TEXT_DIM,
    );

    fonts::draw_centered(
        display, &fonts::value(),
        value, cx, top + VALUE_TOP_OFFSET,
        value_color,
    );
}

// -- circle_stat -------------------------------------------------------------

/// Gap between the bottom of the circle and the top of its label.
const CIRCLE_STAT_LABEL_GAP: i32 = 12;

/// Render a "dark circle containing a value, with a small caption
/// below" stat element at `(cx, cy)` with the given `radius`.
///
/// Matches the reference watch-face pattern: a `primitives::circle_button`
/// filled with `theme::PANEL_BG` and outlined in `theme::AMBER_DIM`, the
/// `value` text centered inside (body font) in `value_color`, and the
/// `label` caption drawn centered immediately below the circle in
/// `theme::TEXT_DIM`.
///
/// Currently unused - the clock face ended up with icon glyphs in
/// its circles instead of values, per the reference visual. Kept
/// as a ready helper for Stopwatch / Timer that will show running
/// times or similar values in the same circle style.
#[allow(dead_code)]
pub fn circle_stat<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    cx: i32, cy: i32, radius: i32,
    value: &str,
    value_color: Rgb565,
    label: &str,
) {
    primitives::circle_button(
        display,
        cx, cy, radius,
        theme::PANEL_BG,
        Some(theme::AMBER_DIM),
    );

    let bounds = Rectangle::new(
        Point::new(cx - radius, cy - radius),
        Size::new((radius * 2) as u32, (radius * 2) as u32),
    );
    fonts::draw_centered_in_rect(
        display, &fonts::body(),
        value, bounds,
        value_color,
    );

    fonts::draw_centered(
        display, &fonts::caption(),
        label, cx, cy + radius + CIRCLE_STAT_LABEL_GAP,
        theme::TEXT_DIM,
    );
}
