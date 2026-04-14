//! Shared layout grammar for card-list screens.
//!
//! These constants define the positional defaults every standard
//! Settings-style screen uses - where the header bar sits, how wide
//! a card is, how much space between cards, where the first card
//! starts. Screens import from here instead of declaring their own
//! copies so the vertical rhythm stays consistent as the user moves
//! between screens and sub-views.
//!
//! Split from `theme` on purpose:
//!
//! * [`theme`] is about the **visual language**: palette, fonts,
//!   physical screen constants, bezel geometry. Colors and sizes
//!   of things.
//! * [`layout`] is about **where things go**: rect helpers,
//!   content positioning, standard card stack geometry. Composition
//!   of things.
//!
//! A palette tweak doesn't force re-reading this file, and a layout
//! tweak doesn't force re-reading the palette. Screens pulling in
//! `use crate::ui::layout::*` get the whole grammar at once.
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
        && py >= h.top_left.y
        && py < h.top_left.y + h.size.height as i32
}
