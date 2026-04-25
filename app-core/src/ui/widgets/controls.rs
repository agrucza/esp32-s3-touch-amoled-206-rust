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
    primitives::{Line, PrimitiveStyle, Rectangle},
    Drawable,
};

use crate::ui::{fonts, theme};
use crate::ui::widgets::containers::chamfered_panel;

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

// -- chamfered_button --------------------------------------------------------

/// Notch size for chamfered buttons. Smaller than the panel notch
/// (10) so buttons read as a different category of surface.
pub const BUTTON_NOTCH: i32 = 8;

/// Variant of a [`chamfered_button`].
#[derive(Clone, Copy)]
#[allow(dead_code)]
pub enum ButtonVariant {
    /// Filled accent background, black text. The "primary action"
    /// affordance (PURGE, RESTORE, CONFIRM).
    Primary,
    /// Steel border, transparent body, FG label. The "cancel /
    /// non-destructive" affordance.
    Ghost,
}

/// Draw a chamfered hex button into `rect`.
///
/// `Primary`: interior filled with `accent`, TL+BR corners carved
///   black to expose the chamfer; label drawn in black so it reads
///   as printed on the colored body.
/// `Ghost`: outline-only in steel, label in `theme::FG`.
pub fn chamfered_button<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    rect: Rectangle,
    label: &str,
    variant: ButtonVariant,
    accent: Rgb565,
) {
    let notch = BUTTON_NOTCH;
    match variant {
        ButtonVariant::Primary => {
            // Fill the whole rect with accent, then carve TL and BR
            // chamfer corners back to BG so the hex shape reads.
            Rectangle::new(rect.top_left, rect.size)
                .into_styled(PrimitiveStyle::with_fill(accent))
                .draw(display).ok();

            let x = rect.top_left.x;
            let y = rect.top_left.y;
            let r = x + rect.size.width as i32 - 1;
            let b = y + rect.size.height as i32 - 1;
            for i in 0..notch {
                // TL chamfer
                Line::new(
                    Point::new(x + i, y),
                    Point::new(x, y + i),
                )
                .into_styled(PrimitiveStyle::with_stroke(theme::BG, 1))
                .draw(display).ok();
                // BR chamfer
                Line::new(
                    Point::new(r - i, b),
                    Point::new(r, b - i),
                )
                .into_styled(PrimitiveStyle::with_stroke(theme::BG, 1))
                .draw(display).ok();
            }

            // Outline (so the chamfer reads as a sharp edge, not a
            // jagged carve).
            chamfered_panel(display, rect, notch, accent, 1);

            fonts::draw_centered_in_rect(
                display, &fonts::caption(), label, rect, theme::BG,
            );
        }
        ButtonVariant::Ghost => {
            // No fill - just the chamfered outline in steel and the
            // label in FG.
            chamfered_panel(display, rect, notch, theme::STEEL, 1);
            let _ = accent; // unused for Ghost
            fonts::draw_centered_in_rect(
                display, &fonts::caption(), label, rect, theme::FG,
            );
        }
    }
}
