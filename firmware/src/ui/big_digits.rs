//! Large seven-segment style digit renderer.
//!
//! Draws chunky blocky digits using filled rectangles - cyberpunk style.
//! Each digit is drawn on a grid of `W` x `H` pixels with `T` thick segments.

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{Point, Size},
    pixelcolor::Rgb565,
    prelude::Primitive,
    primitives::{Rectangle, PrimitiveStyle},
    Drawable,
};

// Digit dimensions
const W: i32 = 24;   // digit width
const H: i32 = 44;   // digit height
const T: i32 = 5;    // segment thickness
const GAP: i32 = 4;  // gap between digits
const COLON_W: i32 = T + 8; // colon width including gaps

/// Total pixel width of a HH:MM display.
pub const TIME_WIDTH: i32 = 4 * (W + GAP) + COLON_W - GAP;

// Seven segment layout:
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

/// Draw a single large digit at position (x, y).
fn draw_digit<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, x: i32, y: i32, digit: u8, color: Rgb565,
) {
    let s = SEGMENTS[digit as usize];
    let inner_w = W - T * 2;
    let half_h = (H - T * 3) / 2;

    // A - top horizontal
    if s & 0x01 != 0 {
        draw_segment(display, x + T, y, inner_w, T, color);
    }
    // B - top-right vertical
    if s & 0x02 != 0 {
        draw_segment(display, x + W - T, y + T, T, half_h, color);
    }
    // C - bottom-right vertical
    if s & 0x04 != 0 {
        draw_segment(display, x + W - T, y + T * 2 + half_h, T, half_h, color);
    }
    // D - bottom horizontal
    if s & 0x08 != 0 {
        draw_segment(display, x + T, y + H - T, inner_w, T, color);
    }
    // E - bottom-left vertical
    if s & 0x10 != 0 {
        draw_segment(display, x, y + T * 2 + half_h, T, half_h, color);
    }
    // F - top-left vertical
    if s & 0x20 != 0 {
        draw_segment(display, x, y + T, T, half_h, color);
    }
    // G - middle horizontal
    if s & 0x40 != 0 {
        draw_segment(display, x + T, y + T + half_h, inner_w, T, color);
    }
}

/// Draw a colon separator (two squares).
fn draw_colon<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, x: i32, y: i32, color: Rgb565,
) {
    let dot = T + 1;
    let cx = x + 2;
    draw_segment(display, cx, y + H / 3 - dot / 2, dot, dot, color);
    draw_segment(display, cx, y + H * 2 / 3 - dot / 2, dot, dot, color);
}

/// Draw `HH:MM` at position (x, y) using large seven-segment digits.
///
/// Returns the total width used.
pub fn draw_time<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, x: i32, y: i32, hour: u8, minute: u8, color: Rgb565,
) -> i32 {
    let mut cx = x;

    // Hours
    draw_digit(display, cx, y, hour / 10, color);
    cx += W + GAP;
    draw_digit(display, cx, y, hour % 10, color);
    cx += W + GAP;

    // Colon
    draw_colon(display, cx, y, color);
    cx += T + GAP + 4;

    // Minutes
    draw_digit(display, cx, y, minute / 10, color);
    cx += W + GAP;
    draw_digit(display, cx, y, minute % 10, color);
    cx += W;

    cx - x
}
