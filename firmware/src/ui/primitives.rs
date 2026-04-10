//! Drawing primitives for the Mankind Divided inspired UI.
//!
//! - `title_box` - signature MD screen/section title, uppercase text in an
//!   outlined rectangle
//! - `bracket_corners` - L-shaped corner decorations for content panels
//! - `diamond` - rhombus marker (carousel dots, notifications)
//! - `triangle_bullet` - small right-pointing selection marker
//! - `section_rule` - thin horizontal divider
//! - `text_button` - outlined rectangle containing uppercase text,
//!   returns its hit rect for tap detection
//! - `flat_bar` - solid progress bar (replaces segmented cyberpunk bars)

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{Point, Size},
    mono_font::{ascii, MonoTextStyle, MonoFont},
    pixelcolor::Rgb565,
    prelude::Primitive,
    primitives::{Line, PrimitiveStyle, Rectangle, Triangle},
    text::{Baseline, Text},
    Drawable,
};

use super::theme;

// -- Title box ---------------------------------------------------------------

/// Draw the signature MD title: uppercase text inside an outlined rectangle.
/// Returns the bounding rectangle so callers can lay out adjacent elements.
///
/// `font` selects the size. Padding is proportional to the font height.
pub fn title_box<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    x: i32,
    y: i32,
    text: &str,
    fg: Rgb565,
    border: Rgb565,
    font: &MonoFont<'static>,
) -> Rectangle {
    let text_style = MonoTextStyle::new(font, fg);
    let ch_w = font.character_size.width as i32;
    let ch_h = font.character_size.height as i32;
    let pad_x = 8;
    let pad_y = 4;
    let w = text.len() as i32 * ch_w + pad_x * 2;
    let h = ch_h + pad_y * 2;

    let rect = Rectangle::new(Point::new(x, y), Size::new(w as u32, h as u32));
    rect.into_styled(PrimitiveStyle::with_stroke(border, 1))
        .draw(display).ok();

    Text::with_baseline(
        text,
        Point::new(x + pad_x, y + pad_y),
        text_style,
        Baseline::Top,
    )
    .draw(display).ok();

    rect
}

// -- Bracket corners ---------------------------------------------------------

/// Draw L-shaped bracket markers at the four corners of a rectangle.
/// Replaces the HR-era `cut_box` - cheaper, cleaner, and the defining
/// frame motif of Mankind Divided content panels.
pub fn bracket_corners<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    x: i32, y: i32, w: i32, h: i32,
    arm: i32,
    color: Rgb565,
) {
    let s = PrimitiveStyle::with_stroke(color, 1);
    let x2 = x + w - 1;
    let y2 = y + h - 1;

    // Top-left
    Line::new(Point::new(x, y), Point::new(x + arm, y)).into_styled(s).draw(display).ok();
    Line::new(Point::new(x, y), Point::new(x, y + arm)).into_styled(s).draw(display).ok();
    // Top-right
    Line::new(Point::new(x2 - arm, y), Point::new(x2, y)).into_styled(s).draw(display).ok();
    Line::new(Point::new(x2, y), Point::new(x2, y + arm)).into_styled(s).draw(display).ok();
    // Bottom-left
    Line::new(Point::new(x, y2 - arm), Point::new(x, y2)).into_styled(s).draw(display).ok();
    Line::new(Point::new(x, y2), Point::new(x + arm, y2)).into_styled(s).draw(display).ok();
    // Bottom-right
    Line::new(Point::new(x2 - arm, y2), Point::new(x2, y2)).into_styled(s).draw(display).ok();
    Line::new(Point::new(x2, y2 - arm), Point::new(x2, y2)).into_styled(s).draw(display).ok();
}

// -- Diamond marker ----------------------------------------------------------

/// Draw a rhombus marker centered at (cx, cy) with half-diagonal `size`.
/// When `filled` is true the interior is filled via horizontal scanlines.
pub fn diamond<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    cx: i32, cy: i32, size: i32,
    color: Rgb565,
    filled: bool,
) {
    if filled {
        // Fill with horizontal lines shrinking toward the tips.
        let stroke = PrimitiveStyle::with_stroke(color, 1);
        for dy in -size..=size {
            let w = size - dy.abs();
            Line::new(
                Point::new(cx - w, cy + dy),
                Point::new(cx + w, cy + dy),
            ).into_styled(stroke).draw(display).ok();
        }
    } else {
        let s = PrimitiveStyle::with_stroke(color, 1);
        let top = Point::new(cx, cy - size);
        let right = Point::new(cx + size, cy);
        let bot = Point::new(cx, cy + size);
        let left = Point::new(cx - size, cy);
        Line::new(top, right).into_styled(s).draw(display).ok();
        Line::new(right, bot).into_styled(s).draw(display).ok();
        Line::new(bot, left).into_styled(s).draw(display).ok();
        Line::new(left, top).into_styled(s).draw(display).ok();
    }
}

// -- Triangle bullet ---------------------------------------------------------

/// Small right-pointing filled triangle, used as the MD list selection
/// marker. Top-left corner of the bounding box is (x, y); default size
/// is 6x10 pixels.
pub fn triangle_bullet<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, x: i32, y: i32, color: Rgb565,
) {
    Triangle::new(
        Point::new(x, y),
        Point::new(x, y + 10),
        Point::new(x + 7, y + 5),
    )
    .into_styled(PrimitiveStyle::with_fill(color))
    .draw(display).ok();
}

// -- Section rule ------------------------------------------------------------

/// Thin 1-px horizontal rule. Used as section divider under headers and
/// between rows.
pub fn section_rule<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, x: i32, y: i32, w: i32, color: Rgb565,
) {
    Line::new(Point::new(x, y), Point::new(x + w - 1, y))
        .into_styled(PrimitiveStyle::with_stroke(color, 1))
        .draw(display).ok();
}

// -- Text button -------------------------------------------------------------

/// Outlined rectangle with centered uppercase text. Returns the hit
/// rectangle so the caller can hit-test touch events against it.
pub fn text_button<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    x: i32, y: i32,
    text: &str,
    fg: Rgb565,
    border: Rgb565,
) -> Rectangle {
    let font = &ascii::FONT_10X20;
    let text_style = MonoTextStyle::new(font, fg);
    let ch_w = font.character_size.width as i32;
    let ch_h = font.character_size.height as i32;
    let pad_x = 14;
    let pad_y = 6;
    let w = text.len() as i32 * ch_w + pad_x * 2;
    let h = ch_h + pad_y * 2;

    let rect = Rectangle::new(Point::new(x, y), Size::new(w as u32, h as u32));
    rect.into_styled(PrimitiveStyle::with_stroke(border, 1))
        .draw(display).ok();

    Text::with_baseline(
        text,
        Point::new(x + pad_x, y + pad_y),
        text_style,
        Baseline::Top,
    )
    .draw(display).ok();

    rect
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
    // Trough
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
