//! Container widgets - surface shapes that content lives inside.
//!
//! Two visual idioms coexist:
//!
//! * **Rounded card** (`card` + `CardStyle`) - legacy rounded panel
//!   with optional status dot. Still used by stopwatch, timer, alarm,
//!   status, and the settings leaf sub-views that carry tabular
//!   diagnostic data.
//! * **Chamfered hex panel** (`chamfered_panel`, `tile`, `tag_label`) -
//!   sharp Nightwatch surfaces traced as 6-line outlines with a
//!   45-degree notch on the TL and BR corners. Used by the watch face,
//!   app drawer, quick access, and the settings index.
//!
//! Containers never draw their own content - body helpers and screen
//! code place content into the same rect after the container is drawn.

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{Point, Size},
    pixelcolor::Rgb565,
    prelude::Primitive,
    primitives::{Circle, Line, PrimitiveStyle, Rectangle},
    Drawable,
};

use crate::ui::{fonts, primitives::rounded_panel, theme};

// -- Widget-local layout constants -------------------------------------------

/// Corner radius for rounded cards. Tuned against the "All Bookings"
/// reference visual.
pub const CARD_RADIUS: u32 = 24;

/// Diameter of the status-accent dot at the right edge of a card.
pub const STATUS_DOT_DIAMETER: i32 = 12;

/// Horizontal inset of the status-dot center from the card's right
/// edge.
pub const STATUS_DOT_INSET: i32 = 22;

/// Chamfer notch size for Nightwatch panels and tiles. Matches the
/// spec's default 10 px corner cut.
pub const NOTCH: i32 = 10;

/// Height of a tag-label ribbon.
pub const TAG_LABEL_H: i32 = 15;

// -- CardStyle ---------------------------------------------------------------

/// Visual style for a [`card`] container.
#[derive(Debug, Clone, Copy)]
pub struct CardStyle {
    pub bg: Rgb565,
    pub border: Option<Rgb565>,
    pub radius: u32,
    /// Optional accent dot drawn at the right edge, vertically
    /// centered (e.g. PASS/FAIL on diagnostics).
    pub status_dot: Option<Rgb565>,
}

impl CardStyle {
    /// Standard filled card: dark ink panel, no border, generous
    /// corner radius, no status accent.
    pub const DEFAULT: Self = Self {
        bg: theme::INK,
        border: None,
        radius: CARD_RADIUS,
        status_dot: None,
    };

    /// Builder-style helper: clone `self` with a status dot color
    /// applied.
    pub const fn with_status_dot(mut self, color: Rgb565) -> Self {
        self.status_dot = Some(color);
        self
    }
}

// -- card --------------------------------------------------------------------

/// Draw a rounded card container into `rect` with the given style.
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

// -- chamfered_panel ---------------------------------------------------------

/// Draw the 6-line outline of a chamfered hex panel: a rectangle with
/// `notch` px cut off the top-left and bottom-right corners.
///
/// Traces outline only - no fill. Screens that want a filled interior
/// fill a plain `Rectangle` first.
///
/// ```text
///       notch
///      ┌────────────────┐
///     ╱                 │
///    │                  │
///    │                  │
///    │                 ╱
///    └────────────────┘
///                notch
/// ```
pub fn chamfered_panel<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    rect: Rectangle,
    notch: i32,
    color: Rgb565,
    stroke_width: u32,
) {
    let x = rect.top_left.x;
    let y = rect.top_left.y;
    let w = rect.size.width as i32;
    let h = rect.size.height as i32;
    let r = x + w - 1;
    let b = y + h - 1;

    let style = PrimitiveStyle::with_stroke(color, stroke_width);

    Line::new(Point::new(x + notch, y), Point::new(r, y))
        .into_styled(style).draw(display).ok();
    Line::new(Point::new(r, y), Point::new(r, b - notch))
        .into_styled(style).draw(display).ok();
    Line::new(Point::new(r, b - notch), Point::new(r - notch, b))
        .into_styled(style).draw(display).ok();
    Line::new(Point::new(r - notch, b), Point::new(x, b))
        .into_styled(style).draw(display).ok();
    Line::new(Point::new(x, b), Point::new(x, y + notch))
        .into_styled(style).draw(display).ok();
    Line::new(Point::new(x, y + notch), Point::new(x + notch, y))
        .into_styled(style).draw(display).ok();
}

// -- tag_label ---------------------------------------------------------------

/// Draw a tag-label flag - a filled accent-colored rectangle with a
/// chamfered bottom-right corner (the classic "flag" shape) and an
/// optional chamfered top-left corner so the tag fits flush against
/// a parent panel's matching TL chamfer.
///
/// `left_x` / `top_y` are the flag's top-left corner. `tl_notch = 0`
/// gives a square TL (tag hangs beside a panel's chamfer); `tl_notch
/// > 0` carves a matching chamfer so the tag can nest inside a
/// chamfered panel corner (pass the panel's own `NOTCH`).
///
/// Text is always drawn in black so it reads as printed on the
/// colored ribbon.
pub fn tag_label<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    left_x: i32, top_y: i32,
    text: &str,
    color: Rgb565,
    tl_notch: i32,
) {
    let font = fonts::caption();
    let text_w = fonts::measure_width(&font, text);
    let w = text_w + 12 + tl_notch;
    let h = TAG_LABEL_H;

    Rectangle::new(
        Point::new(left_x, top_y),
        Size::new(w as u32, h as u32),
    )
    .into_styled(PrimitiveStyle::with_fill(color))
    .draw(display).ok();

    let br_chamfer = 5i32;
    let r = left_x + w - 1;
    let b = top_y + h - 1;
    for i in 0..br_chamfer {
        Line::new(
            Point::new(r - i, b),
            Point::new(r, b - i),
        )
        .into_styled(PrimitiveStyle::with_stroke(theme::BG, 1))
        .draw(display).ok();
    }

    if tl_notch > 0 {
        for i in 0..tl_notch {
            Line::new(
                Point::new(left_x + i, top_y),
                Point::new(left_x, top_y + i),
            )
            .into_styled(PrimitiveStyle::with_stroke(theme::BG, 1))
            .draw(display).ok();
        }
    }

    let text_rect = Rectangle::new(
        Point::new(left_x + tl_notch / 2, top_y),
        Size::new((w - tl_notch / 2) as u32, h as u32),
    );
    fonts::draw_centered_in_rect(display, &font, text, text_rect, theme::BG);
}

// -- tile --------------------------------------------------------------------

/// Draw a chamfered tile suitable for app-grid / toggle-grid use: hex
/// outline in `border` color with an icon + caption inside.
///
/// `stroke_width` controls border thickness - pass 1 for a regular
/// tile, 2 to emphasise the tile as active ("launched from here")
/// without changing the color.
pub fn tile<D, F>(
    display: &mut D,
    rect: Rectangle,
    border: Rgb565,
    stroke_width: u32,
    icon: F,
    icon_color: Rgb565,
    caption: &str,
)
where
    D: DrawTarget<Color = Rgb565>,
    F: FnOnce(&mut D, i32, i32, Rgb565),
{
    chamfered_panel(display, rect, NOTCH, border, stroke_width);

    let x = rect.top_left.x;
    let y = rect.top_left.y;
    let w = rect.size.width as i32;
    let h = rect.size.height as i32;

    let icon_cx = x + w / 2;
    let icon_cy = y + h * 42 / 100;
    icon(display, icon_cx, icon_cy, icon_color);

    let font = fonts::caption();
    fonts::draw_centered(
        display, &font, caption,
        icon_cx, y + h - 18,
        theme::FG,
    );
}
