//! Large seven-segment style digit renderer.
//!
//! Draws blocky digits using filled rectangles. `DigitStyle` controls
//! the size via width, height, segment thickness, and inter-digit gap.
//! Three presets are provided: SMALL (header glance clock), NORMAL
//! (original size), LARGE (dedicated clock face).

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{Point, Size},
    pixelcolor::Rgb565,
    prelude::Primitive,
    primitives::{Rectangle, PrimitiveStyle},
    Drawable,
};

/// Dimensions of a single digit (and the inter-digit gap).
#[derive(Debug, Clone, Copy)]
pub struct DigitStyle {
    /// Digit width.
    pub w: i32,
    /// Digit height.
    pub h: i32,
    /// Segment thickness.
    pub t: i32,
    /// Horizontal gap between digits.
    pub gap: i32,
}

impl DigitStyle {
    /// Small glance-size clock for the compact header.
    pub const SMALL:  Self = Self { w: 16, h: 28, t: 4, gap: 3 };
    /// Original size used elsewhere.
    pub const NORMAL: Self = Self { w: 24, h: 44, t: 5, gap: 4 };
    /// Big clock face size for the dedicated Clock screen.
    pub const LARGE:  Self = Self { w: 48, h: 88, t: 9, gap: 6 };

    /// Total pixel width of a HH:MM display in this style.
    ///
    /// Matches the advances done inside `draw_time` exactly so that
    /// centering calculations line up with what gets drawn.
    pub const fn time_width(&self) -> i32 {
        // Per draw_time advances:
        //   4 digits + 4 inter-element gaps + colon core (t) + 4
        4 * self.w + 4 * self.gap + self.t + 4
    }
}

// Seven-segment layout:
//  _AAA_
// |     |
// F     B
// |     |
//  _GGG_
// |     |
// E     C
// |     |
//  _DDD_
//
// Segments as bitmask: A=0x01 B=0x02 C=0x04 D=0x08 E=0x10 F=0x20 G=0x40

const SEGMENTS: [u8; 10] = [
    0x3F, // 0: A B C D E F
    0x06, // 1: B C
    0x5B, // 2: A B D E G
    0x4F, // 3: A B C D G
    0x66, // 4: B C F G
    0x6D, // 5: A C D F G
    0x7D, // 6: A C D E F G
    0x07, // 7: A B C
    0x7F, // 8: A B C D E F G
    0x6F, // 9: A B C D F G
];

fn draw_segment<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, x: i32, y: i32, w: i32, h: i32, color: Rgb565,
) {
    Rectangle::new(Point::new(x, y), Size::new(w as u32, h as u32))
        .into_styled(PrimitiveStyle::with_fill(color))
        .draw(display).ok();
}

/// Draw a single digit at position (x, y).
fn draw_digit<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, x: i32, y: i32, digit: u8, color: Rgb565, s: &DigitStyle,
) {
    let segs = SEGMENTS[digit as usize];
    let inner_w = s.w - s.t * 2;
    let half_h = (s.h - s.t * 3) / 2;

    if segs & 0x01 != 0 { draw_segment(display, x + s.t, y, inner_w, s.t, color); }
    if segs & 0x02 != 0 { draw_segment(display, x + s.w - s.t, y + s.t, s.t, half_h, color); }
    if segs & 0x04 != 0 { draw_segment(display, x + s.w - s.t, y + s.t * 2 + half_h, s.t, half_h, color); }
    if segs & 0x08 != 0 { draw_segment(display, x + s.t, y + s.h - s.t, inner_w, s.t, color); }
    if segs & 0x10 != 0 { draw_segment(display, x, y + s.t * 2 + half_h, s.t, half_h, color); }
    if segs & 0x20 != 0 { draw_segment(display, x, y + s.t, s.t, half_h, color); }
    if segs & 0x40 != 0 { draw_segment(display, x + s.t, y + s.t + half_h, inner_w, s.t, color); }
}

/// Draw a colon separator (two square dots).
fn draw_colon<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, x: i32, y: i32, color: Rgb565, s: &DigitStyle,
) {
    let dot = s.t + 1;
    let cx = x + 2;
    draw_segment(display, cx, y + s.h / 3 - dot / 2, dot, dot, color);
    draw_segment(display, cx, y + s.h * 2 / 3 - dot / 2, dot, dot, color);
}

/// Draw HH:MM at (x, y) using the given digit style. Returns the
/// actual pixel width used.
pub fn draw_time<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, x: i32, y: i32, hour: u8, minute: u8, color: Rgb565,
    style: &DigitStyle,
) -> i32 {
    let mut cx = x;

    // Hours
    draw_digit(display, cx, y, hour / 10, color, style);
    cx += style.w + style.gap;
    draw_digit(display, cx, y, hour % 10, color, style);
    cx += style.w + style.gap;

    // Colon
    draw_colon(display, cx, y, color, style);
    cx += style.t + style.gap + 4;

    // Minutes
    draw_digit(display, cx, y, minute / 10, color, style);
    cx += style.w + style.gap;
    draw_digit(display, cx, y, minute % 10, color, style);
    cx += style.w;

    cx - x
}
