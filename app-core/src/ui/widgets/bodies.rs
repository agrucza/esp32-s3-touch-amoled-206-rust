//! Content body helpers - layouts that draw into a `Rectangle` or
//! at a coordinate.
//!
//! Body helpers are the "what goes inside" counterpart to
//! [`containers`]. They take a rect (or center point) and draw a
//! specific content layout into it. They don't own state, don't
//! draw backgrounds, don't care what (if anything) is under them.
//! Use them composed with a [`card`] for the standard look, or
//! standalone on a bare rect for inline values without a panel.
//!
//! [`containers`]: super::containers
//! [`card`]: super::containers::card

use embedded_graphics::{
    draw_target::DrawTarget,
    pixelcolor::Rgb565,
    primitives::Rectangle,
};

use crate::ui::{fonts, layout, primitives, theme};

// -- value_body --------------------------------------------------------------

// Tuned against the "All Bookings" reference visual: small grey
// label sitting near the top of the card, large bold value centered
// below it in the lower half. Offsets are from the top of the rect.

/// Vertical position of the label text (top of glyphs), measured
/// from the top of the card rect.
const LABEL_TOP_OFFSET: i32 = 20;

/// Vertical position of the value text (top of glyphs), measured
/// from the top of the card rect.
const VALUE_TOP_OFFSET: i32 = 44;

/// Render a "small grey label over large value" layout into `rect`.
///
/// * `label` is drawn in the [`fonts::body`] style in [`theme::FG_MUTED`],
///   horizontally centered near the top of the rect.
/// * `value` is drawn in the [`fonts::value`] style (bold) in
///   `value_color`, horizontally centered below the label.
///
/// The `value_color` parameter lets the screen tint the value for
/// semantic meaning (white neutral, green pass, red fail, amber
/// warning). Labels always use `FG_MUTED` so the value pops.
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
        theme::FG_MUTED,
    );

    fonts::draw_centered(
        display, &fonts::value(),
        value, cx, top + VALUE_TOP_OFFSET,
        value_color,
    );
}

// -- icon_button -------------------------------------------------------------

/// Render a tappable icon button: a filled circle with an icon
/// glyph inside and a caption label below.
///
/// Uses the standard circle radius, glyph radius, and label gap
/// from [`layout`] so every icon button across the UI is visually
/// consistent.
///
/// This is the shared pattern used by:
/// - Clock home face (hourglass/TIMER, bell/ALARM circles)
/// - Stopwatch (play-pause/START-PAUSE, stop/RESET circles)
/// - Panel app picker (app icon circles with active/inactive state)
///
/// The `glyph` closure receives `(display, cx, cy, glyph_radius,
/// glyph_color)` and should call one of the `glyphs::*` functions.
pub fn icon_button<D, F>(
    display: &mut D,
    cx: i32, cy: i32,
    fill: Rgb565,
    glyph: F,
    glyph_color: Rgb565,
    label: &str,
    label_color: Rgb565,
)
where
    D: DrawTarget<Color = Rgb565>,
    F: FnOnce(&mut D, i32, i32, i32, Rgb565),
{
    primitives::circle_button(
        display, cx, cy,
        layout::CIRCLE_RADIUS, fill, None,
    );

    glyph(display, cx, cy, layout::GLYPH_RADIUS, glyph_color);

    fonts::draw_centered(
        display, &fonts::caption(),
        label,
        cx, cy + layout::CIRCLE_RADIUS + layout::CIRCLE_LABEL_GAP,
        label_color,
    );
}
