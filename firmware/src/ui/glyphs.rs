//! Shared icon glyphs for the UI.
//!
//! Every glyph follows the same signature:
//! `(display, cx, cy, radius, color)` - draw an icon centered at
//! `(cx, cy)` fitting inside the given `radius`, using `color` for
//! all strokes and fills.
//!
//! Screens and the panel picker import from here instead of keeping
//! local copies, so visual tweaks to an icon propagate everywhere.

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{Point, Size},
    pixelcolor::Rgb565,
    prelude::Primitive,
    primitives::{Circle, Line, PrimitiveStyle, Rectangle, Triangle},
    Drawable,
};

// -- Clock / time glyphs ----------------------------------------------------

/// Analog-clock glyph: circle with hour and minute hands, center dot.
pub fn clock<D: DrawTarget<Color = Rgb565>>(
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

/// Stopwatch glyph: a small dial with a button stem on top and a
/// hand pointing up-right. Distinct from the regular clock glyph.
pub fn stopwatch<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, cx: i32, cy: i32, radius: i32, color: Rgb565,
) {
    let thin  = PrimitiveStyle::with_stroke(color, 2);
    let thick = PrimitiveStyle::with_stroke(color, 3);
    let fill  = PrimitiveStyle::with_fill(color);

    // Slightly smaller main dial so there's room for the stem above
    // without the glyph overflowing its allotted radius.
    let dial_r = radius * 4 / 5;

    // Button stem on top: small filled rectangle.
    Rectangle::new(
        Point::new(cx - 3, cy - dial_r - 5),
        Size::new(6, 5),
    ).into_styled(fill).draw(display).ok();

    // Main dial.
    Circle::with_center(Point::new(cx, cy), (dial_r * 2) as u32)
        .into_styled(thin).draw(display).ok();

    // Hand pointing up-right (~1 o'clock).
    Line::new(
        Point::new(cx, cy),
        Point::new(cx + dial_r / 2, cy - dial_r / 2),
    ).into_styled(thick).draw(display).ok();

    // Center cap.
    Circle::with_center(Point::new(cx, cy), 4)
        .into_styled(fill).draw(display).ok();
}

/// Hourglass glyph: two triangular chambers meeting at a point,
/// with flat horizontal caps top and bottom.
pub fn hourglass<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, cx: i32, cy: i32, radius: i32, color: Rgb565,
) {
    let stroke = PrimitiveStyle::with_stroke(color, 3);

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

/// Stylised bell glyph: small handle on top, trapezoidal body
/// flaring to a horizontal base, and a clapper dot below.
pub fn bell<D: DrawTarget<Color = Rgb565>>(
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

// -- Playback control glyphs ------------------------------------------------

/// Filled right-pointing triangle (play icon).
pub fn play<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, cx: i32, cy: i32, radius: i32, color: Rgb565,
) {
    Triangle::new(
        Point::new(cx - radius / 2, cy - radius),
        Point::new(cx - radius / 2, cy + radius),
        Point::new(cx + radius,     cy),
    )
    .into_styled(PrimitiveStyle::with_fill(color))
    .draw(display).ok();
}

/// Two filled vertical bars (pause icon).
pub fn pause<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, cx: i32, cy: i32, radius: i32, color: Rgb565,
) {
    let bar_w = (radius * 2 / 5).max(3);
    let bar_h = radius * 2;
    let gap = radius / 3;
    let fill = PrimitiveStyle::with_fill(color);
    Rectangle::new(
        Point::new(cx - gap - bar_w, cy - bar_h / 2),
        Size::new(bar_w as u32, bar_h as u32),
    ).into_styled(fill).draw(display).ok();
    Rectangle::new(
        Point::new(cx + gap, cy - bar_h / 2),
        Size::new(bar_w as u32, bar_h as u32),
    ).into_styled(fill).draw(display).ok();
}

/// Filled square (stop / reset icon).
pub fn stop<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, cx: i32, cy: i32, radius: i32, color: Rgb565,
) {
    let side = radius * 7 / 4;
    Rectangle::new(
        Point::new(cx - side / 2, cy - side / 2),
        Size::new(side as u32, side as u32),
    )
    .into_styled(PrimitiveStyle::with_fill(color))
    .draw(display).ok();
}

// -- App launcher glyphs ----------------------------------------------------

/// Three stacked horizontal bars (settings / menu icon).
pub fn settings<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, cx: i32, cy: i32, radius: i32, color: Rgb565,
) {
    let style = PrimitiveStyle::with_stroke(color, 3);
    let half_w = radius * 3 / 4;
    let spacing = radius / 3;
    for row in -1..=1 {
        let y = cy + row * spacing;
        Line::new(
            Point::new(cx - half_w, y),
            Point::new(cx + half_w, y),
        )
        .into_styled(style)
        .draw(display)
        .ok();
    }
}

/// Three vertical bars of increasing height (status / bar-chart icon).
pub fn status<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, cx: i32, cy: i32, radius: i32, color: Rgb565,
) {
    let bar_w = (radius / 3).max(4);
    let gap = 4;
    let total_w = 3 * bar_w + 2 * gap;
    let start_x = cx - total_w / 2;
    let base_y = cy + radius / 2;
    let heights = [radius / 2, radius * 3 / 4, radius];
    let fill = PrimitiveStyle::with_fill(color);

    for (i, h) in heights.iter().enumerate() {
        let x = start_x + i as i32 * (bar_w + gap);
        let y = base_y - *h;
        Rectangle::new(Point::new(x, y), Size::new(bar_w as u32, *h as u32))
            .into_styled(fill).draw(display).ok();
    }
}

/// Four small filled squares in a 2x2 grid (panel / app-grid icon).
pub fn panel<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, cx: i32, cy: i32, radius: i32, color: Rgb565,
) {
    let s = (radius / 3).max(4);
    let gap = 4;
    let offsets = [
        (-(s + gap / 2), -(s + gap / 2)),
        ( gap / 2,       -(s + gap / 2)),
        (-(s + gap / 2),  gap / 2),
        ( gap / 2,        gap / 2),
    ];
    let fill = PrimitiveStyle::with_fill(color);
    for (dx, dy) in offsets.iter() {
        Rectangle::new(
            Point::new(cx + dx, cy + dy),
            Size::new(s as u32, s as u32),
        )
        .into_styled(fill).draw(display).ok();
    }
}
