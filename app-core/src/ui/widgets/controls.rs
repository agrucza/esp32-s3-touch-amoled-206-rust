//! Interactive control widgets - toggles, sliders, chamfered buttons.
//!
//! Controls are the smallest interactive primitives. They know how to
//! draw themselves in each visual state (off/on, pressed, disabled)
//! but don't track state themselves - callers pass the current state
//! each render.

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{Point, Size},
    pixelcolor::Rgb565,
    prelude::Primitive,
    primitives::{PrimitiveStyle, Rectangle},
    Drawable,
};

use crate::ui::theme;

// -- toggle ------------------------------------------------------------------

/// Toggle outer width, per the Nightwatch spec (32 x 16 px).
pub const TOGGLE_W: i32 = 32;
/// Toggle outer height.
pub const TOGGLE_H: i32 = 16;

/// Draw a toggle switch at the given top-left.
///
/// - Off: `INK_3` fill, `STEEL` border, `STEEL_2` pill flush-left.
/// - On: `SIGNAL` fill and border, `BG` pill flush-right.
pub fn toggle<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    top_left: Point,
    on: bool,
) {
    let (bg, border, pill) = if on {
        (theme::SIGNAL, theme::SIGNAL, theme::BG)
    } else {
        (theme::INK_3, theme::STEEL, theme::STEEL_2)
    };

    Rectangle::new(top_left, Size::new(TOGGLE_W as u32, TOGGLE_H as u32))
        .into_styled(PrimitiveStyle::with_fill(bg))
        .draw(display).ok();
    Rectangle::new(top_left, Size::new(TOGGLE_W as u32, TOGGLE_H as u32))
        .into_styled(PrimitiveStyle::with_stroke(border, 1))
        .draw(display).ok();

    let pill_size = 12i32;
    let pill_x = if on {
        top_left.x + TOGGLE_W - pill_size - 1
    } else {
        top_left.x + 1
    };
    let pill_y = top_left.y + (TOGGLE_H - pill_size) / 2;
    Rectangle::new(
        Point::new(pill_x, pill_y),
        Size::new(pill_size as u32, pill_size as u32),
    )
    .into_styled(PrimitiveStyle::with_fill(pill))
    .draw(display).ok();
}
