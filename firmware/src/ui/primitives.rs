//! Non-text drawing primitives for the rounded / pill UI.
//!
//! Text rendering lives in `crate::ui::fonts` (u8g2-fonts wrapped
//! around embedded-graphics). This module is purely shape primitives.
//!
//! - `rounded_panel` - rounded rectangle with optional fill and 1px border
//! - `pill_solid` - filled pill (radius = h/2)
//! - `circle_button` - filled circle with optional outline
//! - `dot_carousel` - row of filled dots with one highlighted entry
//! - `scrollbar_v` - vertical pill-shaped page indicator
//! - `section_rule` - thin horizontal divider
//! - `flat_bar` - solid progress bar (trough + fill)
//! - `battery_color` / `battery_icon` / `battery_warning_frame` - battery
//!   indicator helpers

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{Point, Size},
    pixelcolor::Rgb565,
    prelude::Primitive,
    primitives::{Circle, Line, PrimitiveStyle, PrimitiveStyleBuilder, Rectangle, RoundedRectangle, StrokeAlignment},
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

// -- Circle button -----------------------------------------------------------

/// Filled circle button with an optional outline border (no content
/// drawn inside - the caller renders any icon/text/value on top after
/// this returns). Used as the secondary-action element in the modern
/// smartwatch UI: pair of dark circles below a hero pill.
pub fn circle_button<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    cx: i32, cy: i32,
    radius: i32,
    fill: Rgb565,
    border: Option<Rgb565>,
) {
    let mut sb = PrimitiveStyleBuilder::new().fill_color(fill);
    if let Some(c) = border {
        sb = sb.stroke_color(c).stroke_width(1);
    }
    Circle::with_center(Point::new(cx, cy), (radius * 2) as u32)
        .into_styled(sb.build())
        .draw(display).ok();
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

// -- Vertical scrollbar ------------------------------------------------------

/// Vertical page indicator styled as a pill-shaped scrollbar: a dim
/// track with a brighter thumb whose position and height reflect the
/// active page in a 0..count range. Use for vertically-paginated
/// screens (counterpart to `dot_carousel` for horizontal rows).
///
/// The thumb occupies `1/count` of the track height and snaps to
/// discrete per-page positions, so `count=3, active=0..2` places the
/// thumb at the top, middle, and bottom thirds of the track.
pub fn scrollbar_v<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    x: i32, y: i32, w: i32, h: i32,
    count: usize,
    active_idx: usize,
    active_color: Rgb565,
    dim_color: Rgb565,
) {
    if count == 0 { return; }
    let radius = (w as u32) / 2;

    // Track
    rounded_panel(display, x, y, w, h, radius, Some(dim_color), None);

    // Thumb - height = 1/count of the track, clamped so it stays at
    // least as tall as it is wide (keeps the pill shape readable even
    // when there are many pages).
    let thumb_h = (h / count as i32).max(w);
    let max_off = (h - thumb_h).max(0);
    let steps = (count as i32 - 1).max(1);
    let active = (active_idx as i32).min(count as i32 - 1).max(0);
    let thumb_y = y + (active * max_off) / steps;
    rounded_panel(display, x, thumb_y, w, thumb_h, radius, Some(active_color), None);
}

// -- Section rule ------------------------------------------------------------

/// Thin 1-px horizontal rule. Handy as a divider inside cards or
/// between stacked sections; currently unused but kept as a staple
/// primitive for future layouts.
#[allow(dead_code)]
pub fn section_rule<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, x: i32, y: i32, w: i32, color: Rgb565,
) {
    Line::new(Point::new(x, y), Point::new(x + w - 1, y))
        .into_styled(PrimitiveStyle::with_stroke(color, 1))
        .draw(display).ok();
}

// -- Battery indicators ------------------------------------------------------

/// Pick the right status color for a given battery percentage:
/// neutral white when healthy, amber as a heads-up, red when critical.
pub fn battery_color(percent: u8) -> Rgb565 {
    use super::theme;
    if percent > 50 { theme::TEXT_WHITE }
    else if percent >= 20 { theme::AMBER }
    else { theme::RED }
}

/// Draw a small battery glyph (rectangle + nub) with a color-coded
/// fill level. `(x, y)` is the top-left of the body. Total width
/// including the nub is 33 px; body height is 14 px.
///
/// Currently unused since the clock screen uses circle buttons with
/// text instead, but kept as a ready-to-use battery indicator for
/// any future screen that wants a compact visual.
#[allow(dead_code)]
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
/// Currently unused; kept as a general-purpose bar primitive for
/// future screens that need a linear progress/level indicator.
#[allow(dead_code)]
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
