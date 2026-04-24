//! Container widgets - rounded cards, panels, etc.
//!
//! Containers define the visual frame content lives inside. They
//! never know what's drawn on top of them - body helpers and screen
//! code place content into the same rect after the container is
//! drawn. This keeps container style upgrades (borders, focused
//! states, status accents) centralized in one place.
//!
//! The visual language matches the "All Bookings" reference style:
//! generous corner radius, flat dark grey fill, optional bright
//! status dot at the right edge. Old pre-widget screens (Clock,
//! Status, Panel) keep their existing look and use the primitives
//! module directly.

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::Point,
    pixelcolor::Rgb565,
    prelude::Primitive,
    primitives::{Circle, PrimitiveStyle, Rectangle},
    Drawable,
};

use crate::ui::primitives::rounded_panel;
use crate::ui::theme;

// -- Widget-local layout constants -------------------------------------------
//
// These live in the widget module (not `theme`) so new screens using
// the widgets can evolve independently of the old screens that call
// `theme::CARD_RADIUS` directly. Tuning the new look here never
// ripples back into clock/status/panel.

/// Corner radius for cards in the new widget style. Larger than the
/// legacy `theme::CARD_RADIUS` (16) to match the reference visual.
pub const CARD_RADIUS: u32 = 24;

/// Diameter of the status-accent dot at the right edge of a card.
pub const STATUS_DOT_DIAMETER: i32 = 12;

/// Horizontal inset of the status-dot center from the card's right
/// edge. Tuned so the dot sits cleanly in the margin without
/// crowding body content.
pub const STATUS_DOT_INSET: i32 = 22;

// -- CardStyle ---------------------------------------------------------------

/// Visual style for a [`card`] container.
///
/// Construct a custom style inline, or use one of the provided
/// presets ([`CardStyle::DEFAULT`], [`CardStyle::SELECTED`]).
#[derive(Debug, Clone, Copy)]
pub struct CardStyle {
    /// Panel fill color.
    pub bg: Rgb565,
    /// Optional 1 px border. `None` for no border.
    pub border: Option<Rgb565>,
    /// Corner radius in pixels.
    pub radius: u32,
    /// Optional status accent dot drawn at the right edge of the
    /// card, vertically centered. Used to indicate per-card state
    /// (e.g. PASS/FAIL on diagnostics, unread on a notification
    /// list). `None` for no dot.
    pub status_dot: Option<Rgb565>,
}

impl CardStyle {
    /// Standard filled card: dark grey panel, no border, generous
    /// corner radius, no status accent. This is the default for
    /// virtually every card in the new look. Focus highlighting
    /// (border, glow, background shift) is deliberately deferred
    /// until the first screen actually needs it - we'll add a
    /// preset or a `with_focus(...)` helper then.
    pub const DEFAULT: Self = Self {
        bg: theme::INK,
        border: None,
        radius: CARD_RADIUS,
        status_dot: None,
    };

    /// Builder-style helper: clone `self` with a status dot color
    /// applied. Lets screens keep a single base style and attach
    /// per-row accents without redeclaring the whole struct:
    ///
    /// ```ignore
    /// let style = if result.passed {
    ///     CardStyle::DEFAULT.with_status_dot(theme::GREEN)
    /// } else {
    ///     CardStyle::DEFAULT.with_status_dot(theme::DANGER)
    /// };
    /// card(display, rect, style);
    /// ```
    pub const fn with_status_dot(mut self, color: Rgb565) -> Self {
        self.status_dot = Some(color);
        self
    }
}

// -- card --------------------------------------------------------------------

/// Draw a rounded card container into `rect` with the given style.
///
/// The rect defines both the visible panel and the content region -
/// body helpers drawn on top of the card use the same rect. The
/// optional `status_dot` is drawn after the panel, overlapping the
/// right-margin area (body helpers use horizontal centering or left
/// margins, so no content collision).
pub fn card<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    rect: Rectangle,
    style: CardStyle,
) {
    rounded_panel(
        display,
        rect.top_left.x, rect.top_left.y,
        rect.size.width as i32, rect.size.height as i32,
        style.radius,
        Some(style.bg),
        style.border,
    );

    if let Some(color) = style.status_dot {
        let cx = rect.top_left.x + rect.size.width as i32 - STATUS_DOT_INSET;
        let cy = rect.top_left.y + rect.size.height as i32 / 2;
        Circle::with_center(Point::new(cx, cy), STATUS_DOT_DIAMETER as u32)
            .into_styled(PrimitiveStyle::with_fill(color))
            .draw(display).ok();
    }
}
