//! Chrome widgets - screen-level decorations (header bar, nav hints).
//!
//! Chrome is the stuff that belongs to a screen's outer frame
//! rather than its content: the top bar with a close/back icon and
//! a title, the bottom nav hint, etc. Every screen that uses chrome
//! reserves some rect at the top or bottom of its layout and hands
//! that rect to the chrome helper.
//!
//! The reference visual is the "All Bookings" header: X icon on the
//! left, amber title on the right, no background bar - just icons
//! and text floating on the screen's black background.

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::Point,
    pixelcolor::Rgb565,
    prelude::Primitive,
    primitives::{Line, PrimitiveStyle, Rectangle},
    Drawable,
};

use crate::ui::{fonts, layout, primitives, theme};

// -- Layout constants --------------------------------------------------------

/// Horizontal inset between the screen edge and the icon / title
/// glyphs. Keeps both clear of the bezel arc.
const HEADER_MARGIN: i32 = 28;

/// Half-width (and half-height) of the close/back icon glyph in
/// pixels. Total icon size is twice this.
const HEADER_ICON_HALF: i32 = 8;

/// Stroke width for the close/back icon lines.
const HEADER_ICON_STROKE: u32 = 2;

/// Width of the invisible hit target on the left side of a header
/// bar. Screens use this to classify taps that should fire the
/// close/back action. Deliberately larger than the visible icon so
/// fingers don't have to land precisely.
pub const HEADER_ICON_HIT_WIDTH: i32 = 56;

// -- Icon enum ---------------------------------------------------------------

/// Which icon to draw on the left side of a header bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum HeaderIcon {
    /// No icon. Title-only header.
    None,
    /// Close glyph (X). Canonical "dismiss this screen" affordance.
    Close,
    /// Back chevron (`<`). Canonical "pop one level" affordance.
    Back,
}

// -- header_bar --------------------------------------------------------------

/// Draw a screen-header bar into `rect`.
///
/// * `left_icon` is drawn at the left, vertically centered on the
///   rect, with a configurable horizontal margin.
/// * `title` is drawn right-aligned in [`fonts::headline`] using
///   `title_color`.
///
/// The header does *not* draw a background - it's just glyphs on
/// whatever the screen has already cleared to (usually pure black).
/// This matches the reference visual's floating-chrome look.
///
/// Hit testing: the screen is responsible for catching taps in the
/// left icon area. Use [`HEADER_ICON_HIT_WIDTH`] to size the hit
/// target regardless of which icon is drawn (including `None` - a
/// None header still consumes the left area visually for balance).
pub fn header_bar<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    rect: Rectangle,
    left_icon: HeaderIcon,
    title: &str,
    title_color: Rgb565,
) {
    let left = rect.top_left.x;
    let top = rect.top_left.y;
    let right = left + rect.size.width as i32;
    let cy = top + rect.size.height as i32 / 2;

    // Icon at the left, vertically centered on the bar.
    match left_icon {
        HeaderIcon::None => {}
        HeaderIcon::Close => {
            let cx = left + HEADER_MARGIN + HEADER_ICON_HALF;
            draw_close(display, cx, cy, HEADER_ICON_HALF, theme::TEXT_WHITE);
        }
        HeaderIcon::Back => {
            let cx = left + HEADER_MARGIN + HEADER_ICON_HALF;
            draw_back(display, cx, cy, HEADER_ICON_HALF, theme::TEXT_WHITE);
        }
    }

    // Title right-aligned. The headline font is ~18 px tall; offset
    // the top so the text centers vertically on `cy`.
    fonts::draw_right(
        display, &fonts::headline(),
        title,
        right - HEADER_MARGIN,
        cy - 9,
        title_color,
    );
}

// -- page_scrollbar ----------------------------------------------------------

/// Draw a vertical page-indicator scrollbar at the standard position.
///
/// This is the paginated-screen counterpart to `header_bar` - screens
/// that have multiple pages call this once per render with their page
/// count and active index. All positioning and colors are determined
/// by the layout and theme modules so screens don't carry local
/// scrollbar constants.
///
/// Does nothing when `page_count < 2` (single-page screens don't
/// need a scrollbar).
pub fn page_scrollbar<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    page_count: usize,
    active_page: usize,
) {
    if page_count < 2 { return; }
    primitives::scrollbar_v(
        display,
        layout::SCROLLBAR_X, layout::SCROLLBAR_Y,
        layout::SCROLLBAR_W, layout::SCROLLBAR_H,
        page_count,
        active_page,
        theme::AMBER,
        theme::AMBER_DIM,
    );
}

// -- Internal icon helpers ---------------------------------------------------

fn draw_close<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    cx: i32, cy: i32,
    half: i32,
    color: Rgb565,
) {
    let style = PrimitiveStyle::with_stroke(color, HEADER_ICON_STROKE);
    // Top-left to bottom-right
    Line::new(
        Point::new(cx - half, cy - half),
        Point::new(cx + half, cy + half),
    )
    .into_styled(style)
    .draw(display)
    .ok();
    // Bottom-left to top-right
    Line::new(
        Point::new(cx - half, cy + half),
        Point::new(cx + half, cy - half),
    )
    .into_styled(style)
    .draw(display)
    .ok();
}

fn draw_back<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    cx: i32, cy: i32,
    half: i32,
    color: Rgb565,
) {
    let style = PrimitiveStyle::with_stroke(color, HEADER_ICON_STROKE);
    // Upper diagonal of the `<` chevron
    Line::new(
        Point::new(cx + half, cy - half),
        Point::new(cx - half, cy),
    )
    .into_styled(style)
    .draw(display)
    .ok();
    // Lower diagonal of the `<` chevron
    Line::new(
        Point::new(cx - half, cy),
        Point::new(cx + half, cy + half),
    )
    .into_styled(style)
    .draw(display)
    .ok();
}
