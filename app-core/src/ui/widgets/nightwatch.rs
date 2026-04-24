//! Nightwatch-specific widget primitives.
//!
//! These implement the sharp HUD-panel vocabulary used by the watch
//! face, app grid, and settings screens. They are distinct from the
//! rounded-card widgets in [`containers`] / [`bodies`] / [`chrome`]
//! (which remain in use by stopwatch/timer/alarm/status) so a screen
//! rewrite can swap out its look without affecting unrelated screens.
//!
//! Shape language:
//!
//! * **Chamfered hex outline** - a rectangle with a 45-degree notch
//!   cut out of the top-left and bottom-right corners. Traced as 6
//!   `Line` segments. All panel-like surfaces use this shape; cards
//!   are deliberately absent.
//! * **Tag label** - a short rectangular flag that hangs off the top
//!   edge of a panel, colored in the panel's accent, with the notch
//!   matching the panel's own top-left chamfer.
//! * **Nightwatch header** - chevron-left icon + title (in accent
//!   color) + right-aligned telemetry text + 1-px hairline underline
//!   in the accent color.
//! * **Toggle** - 32x16 pixel box with a small pill inside. Off = ink
//!   fill + steel border + steel-2 pill on the left. On = signal fill
//!   + signal border + black pill on the right.
//! * **Row** - single horizontal line in a vertical list. Left: 16 px
//!   cyan icon. Middle: uppercase label in bone. Right: caller-drawn
//!   control (chevron, toggle, inline mono value). 1 px steel divider
//!   along the bottom.
//!
//! All shapes draw directly onto the supplied `DrawTarget`; they take
//! a `Rectangle` for placement and colors as parameters (no hidden
//! theme lookups inside the widget) so screens can tint them
//! explicitly per accent.
//!
//! [`containers`]: super::containers
//! [`bodies`]: super::bodies
//! [`chrome`]: super::chrome

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{Point, Size},
    pixelcolor::Rgb565,
    prelude::Primitive,
    primitives::{Line, PrimitiveStyle, Rectangle},
    Drawable,
};

use crate::ui::{fonts, theme};

// -- Chamfered panel ---------------------------------------------------------

/// Chamfer notch size in pixels. Matches the spec's default 10 px
/// corner cut. Kept as a single constant so panels, tiles, and pills
/// all share the same notch geometry.
pub const NOTCH: i32 = 10;

/// Draw the 6-line outline of a chamfered hex-panel: a rectangle with
/// `notch` px cut off the top-left and bottom-right corners.
///
/// Traces outline only - no fill. Screens that want a filled interior
/// call this after filling the rect with a plain `Rectangle`.
///
/// Corners (clockwise from top-left notch apex):
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

// -- Tag label ---------------------------------------------------------------

/// Height of a tag-label flag in pixels. Tuned so the uppercase small
/// caption glyphs (`fonts::caption()`, helvR10) have ~2 px padding top
/// and bottom inside the filled flag.
pub const TAG_LABEL_H: i32 = 15;

/// Draw a tag-label flag - a filled accent-colored rectangle with a
/// chamfered bottom-right corner (the classic "flag" shape) and an
/// optional chamfered top-left corner so the tag fits flush against
/// a parent panel's matching TL chamfer.
///
/// `left_x` / `top_y` are the flag's top-left corner. `tl_notch = 0`
/// gives the default square TL (tag hangs beside a panel's chamfer);
/// `tl_notch > 0` carves a matching chamfer so the tag can nest
/// inside a chamfered panel corner (pass the panel's own `NOTCH`).
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
    // Width = text advance + 12 px of horizontal padding (6 each side),
    // plus an extra `tl_notch` on the left so the text doesn't get
    // eaten by the TL chamfer cut.
    let font = fonts::caption();
    let text_w = fonts::measure_width(&font, text);
    let w = text_w + 12 + tl_notch;
    let h = TAG_LABEL_H;

    // Solid body fill.
    Rectangle::new(
        Point::new(left_x, top_y),
        Size::new(w as u32, h as u32),
    )
    .into_styled(PrimitiveStyle::with_fill(color))
    .draw(display).ok();

    // Bottom-right chamfer: 5 short black lines carving a triangular
    // notch out of the filled rectangle.
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

    // Top-left chamfer: same carving trick if the caller asked for
    // one to match a parent panel's TL chamfer.
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

    // Text centered in the body. Shifted right by `tl_notch / 2` so
    // the text visually sits in the trimmed body rather than being
    // pulled left into the chamfer cut.
    let text_rect = Rectangle::new(
        Point::new(left_x + tl_notch / 2, top_y),
        Size::new((w - tl_notch / 2) as u32, h as u32),
    );
    fonts::draw_centered_in_rect(display, &font, text, text_rect, theme::BG);
}

// -- Nightwatch header bar ---------------------------------------------------

/// Height of the Nightwatch header. Titles and telemetry centre
/// vertically on this.
pub const HEADER_H: i32 = 28;

/// Hit-target width for the left chevron. Larger than the visible
/// glyph so fingers don't have to land precisely.
pub const HEADER_ICON_HIT_W: i32 = 110;

/// Vertical slack on the header back-chevron hit zone. Extends the
/// hit rect above and below the visible header bar so finger pads
/// landing slightly outside still register.
pub const HEADER_ICON_HIT_V_SLACK: i32 = 12;

/// Draw the Nightwatch screen header:
///   [chevron-left]  TITLE .............. telemetry
///   ─────────────────────────────────────────────── (1 px hairline)
///
/// - `title` is rendered in the accent color (signal red by default).
/// - `right_text` is rendered in `FG_MUTED` (chrome) to read as passive
///   telemetry.
/// - The hairline underline is drawn at the bottom of `rect` in the
///   accent color.
///
/// The rect width/height defines the bar; the hairline sits on the
/// bottom pixel of the rect so a screen can line content up directly
/// below.
pub fn header<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    rect: Rectangle,
    title: &str,
    right_text: &str,
    accent: Rgb565,
) {
    let x = rect.top_left.x;
    let y = rect.top_left.y;
    let w = rect.size.width as i32;
    let h = rect.size.height as i32;
    // Horizontal pad inside the header rect. Set to 24 so the
    // chevron's leftmost pixel (at cx - 4 = 26 when pad = 24) clears
    // the bezel arc at the header's y-band (safe x starts at ~22 at
    // y = 38 on a 98 px corner-radius screen). Also widens the title's
    // left breathing room and the right-telemetry's right margin.
    let pad = 24i32;

    // Chevron-left glyph, centered vertically.
    let cy = y + h / 2;
    let cx = x + pad + 6;
    let stroke = PrimitiveStyle::with_stroke(accent, 2);
    Line::new(
        Point::new(cx + 4, cy - 6),
        Point::new(cx - 4, cy),
    ).into_styled(stroke).draw(display).ok();
    Line::new(
        Point::new(cx - 4, cy),
        Point::new(cx + 4, cy + 6),
    ).into_styled(stroke).draw(display).ok();

    // Title: bold value font for prominence, accent color.
    let title_font = fonts::value();
    let title_dim = title_font
        .get_rendered_dimensions(title, Point::zero(),
            u8g2_fonts::types::VerticalPosition::Top)
        .ok()
        .and_then(|d| d.bounding_box);
    let title_h = title_dim.map(|b| b.size.height as i32).unwrap_or(18);
    let title_top = y + (h - title_h) / 2;
    fonts::draw_at(
        display, &title_font, title,
        x + pad + 26, title_top,
        accent,
    );

    // Right telemetry text in chrome.
    let tele_font = fonts::caption();
    fonts::draw_right(
        display, &tele_font, right_text,
        x + w - pad, y + h - 12,
        theme::FG_MUTED,
    );

    // 1 px accent hairline along the bottom.
    Line::new(
        Point::new(x, y + h - 1),
        Point::new(x + w - 1, y + h - 1),
    ).into_styled(PrimitiveStyle::with_stroke(accent, 1))
    .draw(display).ok();
}

/// Returns `true` if `(x, y)` lands inside the back-chevron hit zone
/// of a header drawn at `header_rect`. The zone extends well past
/// the visible chevron glyph (110 px wide, +/- 12 px vertical slack)
/// so fingers don't have to land precisely on the 8 px chevron.
pub fn header_icon_hit(x: u16, y: u16, header_rect: Rectangle) -> bool {
    let px = x as i32;
    let py = y as i32;
    let hx = header_rect.top_left.x;
    let hy = header_rect.top_left.y;
    let hh = header_rect.size.height as i32;
    px >= hx && px < hx + HEADER_ICON_HIT_W
        && py >= hy - HEADER_ICON_HIT_V_SLACK
        && py < hy + hh + HEADER_ICON_HIT_V_SLACK
}

// -- Toggle ------------------------------------------------------------------

/// Toggle dimensions per the spec: 32 x 16 box, 1 px border, 12 x 12
/// pill inside with 1 px gap.
pub const TOGGLE_W: i32 = 32;
pub const TOGGLE_H: i32 = 16;

/// Draw a Nightwatch toggle at the given top-left.
///
/// - Off: ink-3 fill, steel border, steel-2 pill flush-left.
/// - On: signal fill, signal border, bg pill flush-right.
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

    // Outer rect: filled + bordered.
    Rectangle::new(top_left, Size::new(TOGGLE_W as u32, TOGGLE_H as u32))
        .into_styled(PrimitiveStyle::with_fill(bg))
        .draw(display).ok();
    Rectangle::new(top_left, Size::new(TOGGLE_W as u32, TOGGLE_H as u32))
        .into_styled(PrimitiveStyle::with_stroke(border, 1))
        .draw(display).ok();

    // Pill: 12 x 12, inset 1 px from the edge it's flush to.
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

// -- Row ---------------------------------------------------------------------

/// Height of one settings-style row.
pub const ROW_H: i32 = 52;

/// Horizontal padding inside a row (left and right edges).
pub const ROW_PAD: i32 = 18;

/// Icon column width including its own right padding. Callers draw
/// their icon glyph at the left edge of this column; the label starts
/// after the column ends.
pub const ROW_ICON_COL_W: i32 = 40;

/// Right-control indicator types for a row. Keeps the hot path
/// allocation-free: callers don't construct custom closures, they
/// pick a variant and the renderer picks the right drawing code.
pub enum RowControl<'a> {
    /// Right-pointing chevron in `color`. Signals "tap to navigate".
    Chevron(Rgb565),
    /// Filled toggle (on/off state).
    Toggle(bool),
    /// Short inline text (mono, small) in `color` - use for status
    /// words like `STABLE` (green), `14/32K` (chrome), etc.
    Inline(&'a str, Rgb565),
}

/// Draw one settings-style row inside `rect`.
///
/// Layout (left to right):
/// - 16 px cyan icon (caller-supplied closure), vertically centered.
/// - Uppercase label in bone (`FG`), vertically centered, starts
///   `ROW_ICON_COL_W` px from the rect's left edge + `ROW_PAD`.
/// - Right control per `control`, right-aligned against
///   `rect.right - ROW_PAD`.
/// - 1 px steel hairline along the bottom of `rect`.
///
/// The icon closure receives `(display, cx, cy, color)`. Width of the
/// drawn icon should fit in a ~16 px box; cx/cy is the glyph center.
pub fn row<D, F>(
    display: &mut D,
    rect: Rectangle,
    icon: F,
    icon_color: Rgb565,
    label: &str,
    control: RowControl,
)
where
    D: DrawTarget<Color = Rgb565>,
    F: FnOnce(&mut D, i32, i32, Rgb565),
{
    let x = rect.top_left.x;
    let y = rect.top_left.y;
    let w = rect.size.width as i32;
    let h = rect.size.height as i32;
    let cy = y + h / 2;

    // Icon, centered in its column.
    let icon_cx = x + ROW_PAD + 8;
    icon(display, icon_cx, cy, icon_color);

    // Label.
    let label_font = fonts::body();
    let label_h = 14;
    fonts::draw_at(
        display, &label_font, label,
        x + ROW_PAD + ROW_ICON_COL_W, cy - label_h / 2,
        theme::FG,
    );

    // Right control.
    match control {
        RowControl::Chevron(color) => {
            let right_x = x + w - ROW_PAD;
            let stroke = PrimitiveStyle::with_stroke(color, 2);
            Line::new(
                Point::new(right_x - 6, cy - 5),
                Point::new(right_x, cy),
            ).into_styled(stroke).draw(display).ok();
            Line::new(
                Point::new(right_x, cy),
                Point::new(right_x - 6, cy + 5),
            ).into_styled(stroke).draw(display).ok();
        }
        RowControl::Toggle(on) => {
            let top = Point::new(
                x + w - ROW_PAD - TOGGLE_W,
                cy - TOGGLE_H / 2,
            );
            toggle(display, top, on);
        }
        RowControl::Inline(text, color) => {
            // Match the label's body font (helvR14) so the two sides
            // of the row read at the same weight. The spec called
            // for 10 px mono here, but at this screen size that's
            // borderline unreadable.
            let font = fonts::body();
            fonts::draw_right(
                display, &font, text,
                x + w - ROW_PAD, cy - 7,
                color,
            );
        }
    }

    // 1 px steel hairline along the full width of the row - reads
    // as a structural rail across the screen rather than a floating
    // underline under the content.
    Line::new(
        Point::new(x, y + h - 1),
        Point::new(x + w - 1, y + h - 1),
    ).into_styled(PrimitiveStyle::with_stroke(theme::STEEL, 1))
    .draw(display).ok();
}

// -- Chamfered tile (for app grid) -------------------------------------------

/// Draw a chamfered tile suitable for app-grid use: hex outline in
/// `border` color, optional icon + caption inside. The icon closure
/// is called with `(display, cx, cy, color)`.
///
/// `stroke_width` controls the border thickness - pass 1 for a
/// regular tile, 2 (or more) to emphasise the tile as active /
/// "launched from here" without changing the color.
///
/// Internal layout:
/// - Icon vertically centered in the upper 60% of the tile.
/// - Caption in uppercase bone below, using `fonts::caption()`.
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

    // Icon: centered horizontally, upper 60% vertically.
    let icon_cx = x + w / 2;
    let icon_cy = y + h * 42 / 100;
    icon(display, icon_cx, icon_cy, icon_color);

    // Caption: centered horizontally, 10 px above the bottom edge.
    let font = fonts::caption();
    fonts::draw_centered(
        display, &font, caption,
        icon_cx, y + h - 18,
        theme::FG,
    );
}
