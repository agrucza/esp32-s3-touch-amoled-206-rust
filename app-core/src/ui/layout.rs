//! Shared layout grammar for all screens.
//!
//! These constants define the positional defaults screens use -
//! where the header bar sits, hero pill dimensions, circle button
//! positions, card stack geometry, scrollbar placement. Screens
//! import from here instead of declaring their own copies so the
//! visual rhythm stays consistent across the UI.
//!
//! Split from `theme` on purpose:
//!
//! * [`theme`] is about the **visual language**: palette, fonts,
//!   physical screen constants, bezel geometry. Colors and sizes
//!   of things.
//! * [`layout`] is about **where things go**: rect helpers,
//!   content positioning, standard screen geometry. Composition
//!   of things.
//!
//! A palette tweak doesn't force re-reading this file, and a layout
//! tweak doesn't force re-reading the palette. Screens pulling in
//! `use crate::ui::layout` get the whole grammar at once.
//!
//! [`theme`]: super::theme

use embedded_graphics::{
    geometry::{Point, Size},
    primitives::Rectangle,
};

use crate::ui::theme;

// -- Header bar --------------------------------------------------------------

/// Top edge of the header bar, measured from the framebuffer top.
/// Clears the bezel arc comfortably.
pub const HEADER_TOP: i32 = 40;

/// Height of the header bar.
pub const HEADER_HEIGHT: i32 = 40;

/// Full-width rect for a screen's header bar. Pass this directly
/// to [`crate::ui::widgets::header_bar`].
pub const fn header_rect() -> Rectangle {
    Rectangle::new(
        Point::new(0, HEADER_TOP),
        Size::new(theme::SCREEN_W as u32, HEADER_HEIGHT as u32),
    )
}

// -- Hero pill ---------------------------------------------------------------

/// Width of the signal hero pill (clock, stopwatch, future timer).
pub const HERO_PILL_W: i32 = 320;

/// Height of the signal hero pill.
pub const HERO_PILL_H: i32 = 130;

/// Top of the hero pill, measured from the framebuffer top.
pub const HERO_PILL_Y: i32 = 160;

/// Left edge of the hero pill (horizontally centered on screen).
pub const HERO_PILL_X: i32 = (theme::SCREEN_W as i32 - HERO_PILL_W) / 2;

/// Rect for centering text inside the hero pill. Pass this to
/// `fonts::draw_centered_in_rect` for visually centered content.
pub const HERO_RECT: Rectangle = Rectangle::new(
    Point::new(HERO_PILL_X, HERO_PILL_Y),
    Size::new(HERO_PILL_W as u32, HERO_PILL_H as u32),
);

// -- Circle button pair ------------------------------------------------------

/// Drawn radius of each bottom circle.
pub const CIRCLE_RADIUS: i32 = 70;

/// Horizontal gap between the two circles (edge-to-edge).
pub const CIRCLE_GAP: i32 = 24;

/// X center of the left circle.
pub const LEFT_CIRCLE_CX: i32 =
    theme::SCREEN_W as i32 / 2 - CIRCLE_GAP / 2 - CIRCLE_RADIUS;

/// X center of the right circle.
pub const RIGHT_CIRCLE_CX: i32 =
    theme::SCREEN_W as i32 / 2 + CIRCLE_GAP / 2 + CIRCLE_RADIUS;

/// Vertical center of both circles.
pub const CIRCLE_CY: i32 = 310 + CIRCLE_RADIUS;

/// Glyph drawing radius - insets the icon inside the circle so it
/// doesn't kiss the border.
pub const GLYPH_RADIUS: i32 = CIRCLE_RADIUS - 37;

/// Gap between the bottom of a circle and the top of its caption.
pub const CIRCLE_LABEL_GAP: i32 = 14;

// -- Circle hit testing ------------------------------------------------------

/// Returns `true` if `(x, y)` lands inside the left circle.
pub fn left_circle_hit(x: u16, y: u16) -> bool {
    let dx = x as i32 - LEFT_CIRCLE_CX;
    let dy = y as i32 - CIRCLE_CY;
    dx * dx + dy * dy <= CIRCLE_RADIUS * CIRCLE_RADIUS
}

/// Returns `true` if `(x, y)` lands inside the right circle.
pub fn right_circle_hit(x: u16, y: u16) -> bool {
    let dx = x as i32 - RIGHT_CIRCLE_CX;
    let dy = y as i32 - CIRCLE_CY;
    dx * dx + dy * dy <= CIRCLE_RADIUS * CIRCLE_RADIUS
}

// -- Hero pill hit testing ---------------------------------------------------

/// Returns `true` if `(x, y)` lands inside the hero pill's bounding rect.
pub fn hero_pill_hit(x: u16, y: u16) -> bool {
    let px = x as i32;
    let py = y as i32;
    px >= HERO_PILL_X
        && px < HERO_PILL_X + HERO_PILL_W
        && py >= HERO_PILL_Y
        && py < HERO_PILL_Y + HERO_PILL_H
}

// -- Card list grammar -------------------------------------------------------

/// Horizontal inset of a card from the screen edge. Picked so the
/// card's rounded corners stay clear of the bezel curve.
pub const CARD_MARGIN_X: i32 = 28;

/// Standard card width: screen width minus the margins on both
/// sides. Used by every full-width card-list screen.
pub const CARD_WIDTH: i32 = theme::SCREEN_W as i32 - CARD_MARGIN_X * 2;

/// Standard card height. Tuned for the two-line "label over value"
/// body layout - smaller feels cramped, larger wastes stack space.
pub const CARD_HEIGHT: i32 = 84;

/// Vertical gap between cards in a stack.
pub const CARD_GAP: i32 = 12;

/// Y of the first card in a card-list screen, measured from the
/// framebuffer top. Leaves breathing room below the header bar.
pub const FIRST_CARD_Y: i32 = HEADER_TOP + HEADER_HEIGHT + 24;

/// Rect for the Nth card in a standard card-list layout, counted
/// from 0. This is the canonical helper every screen with a
/// vertical card stack uses to place cards - pass the return value
/// straight into [`crate::ui::widgets::card`] and
/// [`crate::ui::widgets::value_body`].
///
/// Screens that need a non-standard layout (different card height,
/// multi-column, nested rows) override locally - but prefer this
/// helper whenever possible so the rhythm stays consistent.
pub const fn content_card_rect(index: usize) -> Rectangle {
    let y = FIRST_CARD_Y + index as i32 * (CARD_HEIGHT + CARD_GAP);
    Rectangle::new(
        Point::new(CARD_MARGIN_X, y),
        Size::new(CARD_WIDTH as u32, CARD_HEIGHT as u32),
    )
}

// -- Page scrollbar ----------------------------------------------------------

/// Width of the vertical page-indicator scrollbar (pill track).
pub const SCROLLBAR_W: i32 = 4;

/// X position of the scrollbar. Sits near the right edge, inset
/// enough to clear the bezel arc (screen is 410 wide, bezel corner
/// radius is 98).
pub const SCROLLBAR_X: i32 = theme::SCREEN_W as i32 - 18;

/// Top of the scrollbar track. Aligned with the bezel-safe content
/// band so it spans the full usable height.
pub const SCROLLBAR_Y: i32 = theme::CONTENT_TOP;

/// Height of the scrollbar track.
pub const SCROLLBAR_H: i32 = theme::CONTENT_H;

// -- Header icon hit testing -------------------------------------------------

/// Returns `true` if `(x, y)` lands inside the header bar's left
/// icon region. Used by screens to detect taps on the Close/Back
/// chevron without needing per-icon hit boxes.
pub fn header_icon_hit(x: u16, y: u16) -> bool {
    let h = header_rect();
    let px = x as i32;
    let py = y as i32;
    px >= h.top_left.x
        && px < h.top_left.x + crate::ui::widgets::HEADER_ICON_HIT_WIDTH
        && py < h.top_left.y + h.size.height as i32
}
