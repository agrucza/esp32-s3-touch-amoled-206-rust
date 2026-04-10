//! Drawing primitives for the rounded / pill UI.
//!
//! - `rounded_panel` - rounded rectangle with optional fill and 1px border
//! - `pill_solid` - filled pill (radius = h/2)
//! - `pill_button_rect` - returns the bounding rect a pill button will
//!   use for a given label (so callers can hit-test before drawing)
//! - `pill_button` - draws a solid pill with centered text; returns the
//!   bounding rect
//! - `dot_carousel` - row of filled dots with one highlighted entry
//! - `section_rule` - thin horizontal divider
//! - `flat_bar` - solid progress bar (trough + fill)

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{Point, Size},
    mono_font::{ascii, MonoTextStyle},
    pixelcolor::Rgb565,
    prelude::Primitive,
    primitives::{Circle, Line, PrimitiveStyle, PrimitiveStyleBuilder, Rectangle, RoundedRectangle, StrokeAlignment},
    text::{Baseline, Text},
    Drawable,
};

// -- Rounded panel -----------------------------------------------------------

/// Draw a rounded rectangle with optional fill and border. Returns the
/// axis-aligned bounding rectangle so callers can lay content inside it.
pub fn rounded_panel<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    x: i32, y: i32, w: i32, h: i32,
    radius: u32,
    fill: Option<Rgb565>,
    border: Option<Rgb565>,
) -> Rectangle {
    let rect = Rectangle::new(Point::new(x, y), Size::new(w as u32, h as u32));
    let rr = RoundedRectangle::with_equal_corners(rect, Size::new(radius, radius));

    let mut sb = PrimitiveStyleBuilder::new();
    if let Some(c) = fill { sb = sb.fill_color(c); }
    if let Some(c) = border { sb = sb.stroke_color(c).stroke_width(1); }
    rr.into_styled(sb.build()).draw(display).ok();

    rect
}

// -- Pills -------------------------------------------------------------------

/// Draw a solid pill (rounded rect with radius = h/2).
pub fn pill_solid<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    x: i32, y: i32, w: i32, h: i32,
    fill: Rgb565,
) {
    let radius = (h as u32) / 2;
    rounded_panel(display, x, y, w, h, radius, Some(fill), None);
}

// Shared pill-button geometry so draw + hit-test agree.
const PILL_PAD_X: i32 = 16;
const PILL_PAD_Y: i32 = 6;
const PILL_CH_W: i32 = 10; // FONT_10X20
const PILL_CH_H: i32 = 20;

/// Compute the bounding rectangle a pill button would occupy. Does
/// not draw anything; used for hit-testing without re-rendering.
pub fn pill_button_rect(x: i32, y: i32, text: &str) -> Rectangle {
    let w = text.len() as i32 * PILL_CH_W + PILL_PAD_X * 2;
    let h = PILL_CH_H + PILL_PAD_Y * 2;
    Rectangle::new(Point::new(x, y), Size::new(w as u32, h as u32))
}

/// Draw a solid pill with centered text. Returns the bounding rect.
pub fn pill_button<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    x: i32, y: i32,
    text: &str,
    fg: Rgb565,
    bg: Rgb565,
) -> Rectangle {
    let rect = pill_button_rect(x, y, text);
    let w = rect.size.width as i32;
    let h = rect.size.height as i32;

    // Filled pill background
    let radius = (h as u32) / 2;
    RoundedRectangle::with_equal_corners(
        rect,
        Size::new(radius, radius),
    )
    .into_styled(PrimitiveStyle::with_fill(bg))
    .draw(display).ok();

    // Centered text
    let text_w = text.len() as i32 * PILL_CH_W;
    let text_x = x + (w - text_w) / 2;
    let text_y = y + (h - PILL_CH_H) / 2;
    let style = MonoTextStyle::new(&ascii::FONT_10X20, fg);
    Text::with_baseline(text, Point::new(text_x, text_y), style, Baseline::Top)
        .draw(display).ok();

    rect
}

// -- Dot carousel ------------------------------------------------------------

/// Draw a horizontal row of filled circles, with one highlighted. The
/// entire row is horizontally centered around `cx` at the given `cy`.
pub fn dot_carousel<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    cx: i32, cy: i32,
    count: usize,
    active_idx: usize,
    active_color: Rgb565,
    dim_color: Rgb565,
) {
    if count == 0 { return; }
    let spacing = 12i32;
    let diameter = 6u32;
    let total_w = (count as i32 - 1) * spacing;
    let start_x = cx - total_w / 2;

    for i in 0..count {
        let x = start_x + i as i32 * spacing;
        let color = if i == active_idx { active_color } else { dim_color };
        Circle::with_center(Point::new(x, cy), diameter)
            .into_styled(PrimitiveStyle::with_fill(color))
            .draw(display).ok();
    }
}

// -- Section rule ------------------------------------------------------------

/// Thin 1-px horizontal rule.
pub fn section_rule<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, x: i32, y: i32, w: i32, color: Rgb565,
) {
    Line::new(Point::new(x, y), Point::new(x + w - 1, y))
        .into_styled(PrimitiveStyle::with_stroke(color, 1))
        .draw(display).ok();
}

// -- Battery indicators ------------------------------------------------------

/// Pick the right status color for a given battery percentage.
pub fn battery_color(percent: u8) -> Rgb565 {
    use super::theme;
    if percent > 50 { theme::TEAL }
    else if percent >= 20 { theme::AMBER }
    else { theme::RED }
}

/// Draw a small battery glyph (rectangle + nub) with a color-coded
/// fill level. `(x, y)` is the top-left of the body. Total width
/// including the nub is 33 px; body height is 14 px.
pub fn battery_icon<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    x: i32, y: i32,
    percent: u8,
    border: Rgb565,
) {
    let body_w: i32 = 30;
    let body_h: i32 = 14;
    let nub_w: i32 = 3;
    let nub_h: i32 = 6;

    // Body outline
    Rectangle::new(Point::new(x, y), Size::new(body_w as u32, body_h as u32))
        .into_styled(PrimitiveStyle::with_stroke(border, 1))
        .draw(display).ok();
    // Nub on the right side, vertically centered
    Rectangle::new(
        Point::new(x + body_w, y + (body_h - nub_h) / 2),
        Size::new(nub_w as u32, nub_h as u32),
    )
    .into_styled(PrimitiveStyle::with_fill(border))
    .draw(display).ok();

    // Fill level inset 2 px from the body outline
    let inner_max_w = body_w - 4;
    let inner_h = body_h - 4;
    let pct = percent.min(100) as i32;
    let fill_w = inner_max_w * pct / 100;
    if fill_w > 0 {
        Rectangle::new(
            Point::new(x + 2, y + 2),
            Size::new(fill_w as u32, inner_h as u32),
        )
        .into_styled(PrimitiveStyle::with_fill(battery_color(percent)))
        .draw(display).ok();
    }
}

// -- System overlays ---------------------------------------------------------

/// Draw a colored rounded-rect frame that runs parallel to the bezel
/// curve, used as a system-wide low-battery warning overlay: amber at
/// 10-19%, red below 10%. No frame at 20% or above.
///
/// Important: the frame is *inset* from the framebuffer edges so its
/// corner arc stays well inside the bezel. We can't trace the bezel
/// edge directly because the measured `CORNER_R` is approximate -
/// drawing at the exact bezel radius leaves the corner pixels right
/// at (or just outside) the visible region, where they get clipped.
///
/// By using `(INSET, INSET)` as the rect origin and `CORNER_R - INSET`
/// as the arc radius, the frame's arc center lands at `(CORNER_R,
/// CORNER_R)` - the same center as the bezel arc - and the frame arc
/// runs exactly `INSET` px inside the bezel arc at every point. As
/// long as the bezel is at least `INSET` px more conservative than
/// our radius assumption, the frame is fully visible.
pub fn battery_warning_frame<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    percent: u8,
) {
    use super::theme;
    if percent >= 20 { return; }
    let color = if percent < 10 { theme::RED } else { theme::AMBER };

    const INSET: i32 = 12;
    let w = theme::SCREEN_W as i32 - INSET * 2;
    let h = theme::SCREEN_H as i32 - INSET * 2;
    let radius = (theme::CORNER_R - INSET) as u32;

    let style = PrimitiveStyleBuilder::new()
        .stroke_color(color)
        .stroke_width(3)
        .stroke_alignment(StrokeAlignment::Inside)
        .build();
    RoundedRectangle::with_equal_corners(
        Rectangle::new(Point::new(INSET, INSET), Size::new(w as u32, h as u32)),
        Size::new(radius, radius),
    )
    .into_styled(style)
    .draw(display).ok();
}

// -- Flat progress bar -------------------------------------------------------

/// Solid-fill horizontal progress bar. `value` is clamped to 0..=max.
pub fn flat_bar<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    x: i32, y: i32, w: i32, h: i32,
    value: u16, max: u16,
    fill: Rgb565,
    bg: Rgb565,
) {
    Rectangle::new(Point::new(x, y), Size::new(w as u32, h as u32))
        .into_styled(PrimitiveStyle::with_fill(bg))
        .draw(display).ok();

    if max == 0 { return; }
    let v = value.min(max);
    let fw = (v as i32 * w) / max as i32;
    if fw > 0 {
        Rectangle::new(Point::new(x, y), Size::new(fw as u32, h as u32))
            .into_styled(PrimitiveStyle::with_fill(fill))
            .draw(display).ok();
    }
}
