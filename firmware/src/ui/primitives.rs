//! Cyberpunk-style drawing primitives.
//!
//! Cut-corner boxes, segmented bars, and styled labels
//! drawn with embedded-graphics primitives.

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{Point, Size},
    mono_font::{ascii, MonoTextStyle},
    pixelcolor::Rgb565,
    prelude::Primitive,
    primitives::{Line, PrimitiveStyle, Rectangle},
    text::Text,
    Drawable,
};

use super::theme;

// -- Cut-corner box ----------------------------------------------------------

/// Draw a rectangle with cut corners (cyberpunk style).
///
/// The corners are clipped diagonally by `cut` pixels.
/// ```text
///   ____________________
///  /                    \
/// |                      |
/// |                      |
///  \____________________/
/// ```
pub fn cut_box<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    x: i32, y: i32, w: i32, h: i32,
    color: Rgb565,
    cut: i32,
) {
    let style = PrimitiveStyle::with_stroke(color, 1);

    // Top edge (between cuts)
    Line::new(Point::new(x + cut, y), Point::new(x + w - cut - 1, y))
        .into_styled(style).draw(display).ok();
    // Bottom edge
    Line::new(Point::new(x + cut, y + h - 1), Point::new(x + w - cut - 1, y + h - 1))
        .into_styled(style).draw(display).ok();
    // Left edge
    Line::new(Point::new(x, y + cut), Point::new(x, y + h - cut - 1))
        .into_styled(style).draw(display).ok();
    // Right edge
    Line::new(Point::new(x + w - 1, y + cut), Point::new(x + w - 1, y + h - cut - 1))
        .into_styled(style).draw(display).ok();

    // Corner diagonals
    // Top-left
    Line::new(Point::new(x, y + cut), Point::new(x + cut, y))
        .into_styled(style).draw(display).ok();
    // Top-right
    Line::new(Point::new(x + w - cut - 1, y), Point::new(x + w - 1, y + cut))
        .into_styled(style).draw(display).ok();
    // Bottom-left
    Line::new(Point::new(x, y + h - cut - 1), Point::new(x + cut, y + h - 1))
        .into_styled(style).draw(display).ok();
    // Bottom-right
    Line::new(Point::new(x + w - cut - 1, y + h - 1), Point::new(x + w - 1, y + h - cut - 1))
        .into_styled(style).draw(display).ok();
}

/// Draw a filled header tab with text label (the small colored tag).
///
/// ```text
///  ________
/// | LABEL  \
/// |_________\
/// ```
pub fn header_tab<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    x: i32, y: i32,
    label: &str,
    bg: Rgb565,
    fg: Rgb565,
) {
    let font = MonoTextStyle::new(&ascii::FONT_6X10, fg);
    let text_w = label.len() as i32 * 6;
    let tab_w = text_w + 12;
    let tab_h = 16;

    // Filled background
    Rectangle::new(Point::new(x, y), Size::new(tab_w as u32, tab_h as u32))
        .into_styled(PrimitiveStyle::with_fill(bg))
        .draw(display).ok();

    // Cut corner on top-right
    let cut = 6;
    // Triangle to "cut" the corner - fill with BG color
    for i in 0..cut {
        Line::new(
            Point::new(x + tab_w - cut + i, y),
            Point::new(x + tab_w - 1, y + cut - i - 1),
        ).into_styled(PrimitiveStyle::with_stroke(theme::BG, 1))
        .draw(display).ok();
    }

    // Text
    Text::new(label, Point::new(x + 4, y + 11), font)
        .draw(display).ok();
}

// -- Segmented bar -----------------------------------------------------------

/// Draw a segmented progress bar (cyberpunk health-bar style).
///
/// `value` is 0..=max. Segments are small rectangles with 1px gap.
pub fn segmented_bar<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    x: i32, y: i32, w: i32, h: i32,
    value: u16, max: u16,
    fill_color: Rgb565,
    bg_color: Rgb565,
) {
    let seg_w = 4i32; // segment width
    let gap = 1i32;
    let total_segs = (w + gap) / (seg_w + gap);
    let filled = if max > 0 {
        (value as i32 * total_segs) / max as i32
    } else {
        0
    };

    for i in 0..total_segs {
        let sx = x + i * (seg_w + gap);
        let color = if i < filled { fill_color } else { bg_color };
        Rectangle::new(Point::new(sx, y), Size::new(seg_w as u32, h as u32))
            .into_styled(PrimitiveStyle::with_fill(color))
            .draw(display).ok();
    }
}

/// Draw a horizontal line with small tick marks (scan-line decoration).
pub fn scan_line<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    x: i32, y: i32, w: i32,
    color: Rgb565,
) {
    let style = PrimitiveStyle::with_stroke(color, 1);
    Line::new(Point::new(x, y), Point::new(x + w - 1, y))
        .into_styled(style).draw(display).ok();

    // Small tick marks every 20px
    for i in (0..w).step_by(20) {
        Line::new(Point::new(x + i, y - 1), Point::new(x + i, y + 1))
            .into_styled(style).draw(display).ok();
    }
}

// -- Text helpers ------------------------------------------------------------

/// Draw a label: value pair. Label in dim cyan, value in bright color.
pub fn label_value<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    x: i32, y: i32,
    label: &str,
    value: &str,
    value_color: Rgb565,
) {
    let label_font = MonoTextStyle::new(&ascii::FONT_6X10, theme::DIM_CYAN);
    let value_font = MonoTextStyle::new(&ascii::FONT_6X10, value_color);

    Text::new(label, Point::new(x, y), label_font).draw(display).ok();
    let offset = label.len() as i32 * 6;
    Text::new(value, Point::new(x + offset, y), value_font).draw(display).ok();
}

/// Draw large text (used for main status values).
pub fn large_text<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    x: i32, y: i32,
    text: &str,
    color: Rgb565,
) {
    let font = MonoTextStyle::new(&ascii::FONT_10X20, color);
    Text::new(text, Point::new(x, y), font).draw(display).ok();
}
