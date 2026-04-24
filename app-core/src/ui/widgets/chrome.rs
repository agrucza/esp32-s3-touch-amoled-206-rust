//! Chrome widgets - screen-level decorations.
//!
//! Chrome is stuff that belongs to a screen's outer frame rather than
//! its content: top status bar, title headers, back chevron hit zones,
//! page-indicator scrollbars, bottom home indicators.
//!
//! Two generations coexist:
//!
//! * **Legacy `header_bar`** - X/< icon on the left, right-aligned
//!   title. Floating on pure black, no underline. Used by stopwatch,
//!   timer, alarm, status.
//! * **Nightwatch `header`** - chevron-back + accent title + right
//!   telemetry + 1 px hairline underline. Used by settings.
//! * **Universal status bar + home indicator** - the 18 px top strip
//!   and the 2 px bottom bar drawn on every non-watch-face screen
//!   (app drawer, quick access, settings, and new app screens).

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{Point, Size},
    pixelcolor::Rgb565,
    prelude::Primitive,
    primitives::{Line, PrimitiveStyle, Rectangle},
    Drawable,
};

use crate::ui::{fonts, glyphs, layout, primitives, theme};

// -- Legacy header_bar layout constants --------------------------------------

const HEADER_MARGIN: i32 = 28;
const HEADER_ICON_HALF: i32 = 8;
const HEADER_ICON_STROKE: u32 = 2;

/// Width of the invisible hit target on the left of a `header_bar`.
pub const HEADER_ICON_HIT_WIDTH: i32 = 56;

// -- Nightwatch header constants ---------------------------------------------

/// Height of the Nightwatch header. Titles and telemetry centre
/// vertically on this.
pub const HEADER_H: i32 = 28;

/// Hit-target width for the Nightwatch `header` back chevron.
pub const HEADER_ICON_HIT_W: i32 = 110;

/// Vertical slack on the Nightwatch header hit zone.
pub const HEADER_ICON_HIT_V_SLACK: i32 = 12;

// -- Status bar + home indicator constants -----------------------------------

/// Height of the top status bar drawn on every non-watch-face screen.
pub const STATUS_BAR_H: i32 = 18;

/// Width of the home-indicator bar.
pub const HOME_INDICATOR_W: i32 = 56;

/// Height (thickness) of the home-indicator bar.
pub const HOME_INDICATOR_H: i32 = 2;

// -- Icon enum ---------------------------------------------------------------

/// Which icon to draw on the left side of a [`header_bar`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum HeaderIcon {
    None,
    Close,
    Back,
}

// -- Legacy header_bar -------------------------------------------------------

/// Draw a legacy screen-header bar into `rect`.
///
/// `left_icon` at the left, right-aligned `title`, no background or
/// underline. Used by the card-style screens.
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

    match left_icon {
        HeaderIcon::None => {}
        HeaderIcon::Close => {
            let cx = left + HEADER_MARGIN + HEADER_ICON_HALF;
            draw_close(display, cx, cy, HEADER_ICON_HALF, theme::FG);
        }
        HeaderIcon::Back => {
            let cx = left + HEADER_MARGIN + HEADER_ICON_HALF;
            draw_back(display, cx, cy, HEADER_ICON_HALF, theme::FG);
        }
    }

    fonts::draw_right(
        display, &fonts::headline(),
        title,
        right - HEADER_MARGIN,
        cy - 9,
        title_color,
    );
}

// -- Nightwatch header -------------------------------------------------------

/// Draw the Nightwatch screen header:
/// ```text
/// [chevron-left]  TITLE .............. telemetry
/// ───────────────────────────────────────────────
/// ```
/// The hairline sits on the bottom pixel of the rect so a screen can
/// line content up directly below.
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
    // Horizontal pad inside the header rect. 24 keeps the chevron's
    // leftmost pixel (at cx - 4 = 26) clear of the bezel arc at the
    // header's y-band, and widens title/right-telemetry breathing room.
    let pad = 24i32;

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

    let tele_font = fonts::caption();
    fonts::draw_right(
        display, &tele_font, right_text,
        x + w - pad, y + h - 12,
        theme::FG_MUTED,
    );

    Line::new(
        Point::new(x, y + h - 1),
        Point::new(x + w - 1, y + h - 1),
    ).into_styled(PrimitiveStyle::with_stroke(accent, 1))
    .draw(display).ok();
}

/// Returns `true` if `(x, y)` lands inside the back-chevron hit zone
/// of a Nightwatch `header` drawn at `header_rect`. Zone is wider
/// and taller than the visible chevron so finger pads don't have to
/// land precisely.
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

// -- status_bar --------------------------------------------------------------

/// Draw the top status bar: `HH:MM` on the left, then signal /
/// bluetooth / battery% on the right. Everything drawn in `tint`
/// (signal default, cyan on Quick Access, yellow on Notifications, ...).
///
/// A 1 px hairline in `tint` runs along the bottom so the bar reads
/// as separated from screen content below. `x_inset` pulls the
/// left/right content away from the bezel arc; the bar itself spans
/// full screen width so the hairline reaches edge to edge.
pub fn status_bar<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    y: i32,
    time_text: &str,
    battery_pct: Option<u8>,
    tint: Rgb565,
    x_inset: i32,
) {
    use core::fmt::Write;
    let screen_w = theme::SCREEN_W as i32;
    let h = STATUS_BAR_H;
    let cy = y + h / 2;

    let font = fonts::caption();
    fonts::draw_at(
        display, &font, time_text,
        x_inset, y + 3,
        tint,
    );

    let gap = 5i32;
    let icon_r = 4i32;
    let mut buf: heapless::String<8> = heapless::String::new();
    if let Some(pct) = battery_pct {
        let _ = write!(buf, "{}%", pct);
    } else {
        let _ = buf.push_str("--");
    }
    let pct_w = fonts::measure_width(&font, buf.as_str());
    let right_x = screen_w - x_inset;

    fonts::draw_at(
        display, &font, buf.as_str(),
        right_x - pct_w, y + 3,
        tint,
    );

    let bt_cx = right_x - pct_w - gap - icon_r;
    glyphs::bluetooth_small(display, bt_cx, cy, icon_r, tint);

    let sig_cx = bt_cx - icon_r * 2 - gap;
    glyphs::signal_small(display, sig_cx, cy, icon_r, tint);

    Line::new(
        Point::new(0, y + h - 1),
        Point::new(screen_w - 1, y + h - 1),
    ).into_styled(PrimitiveStyle::with_stroke(tint, 1))
    .draw(display).ok();
}

// -- home_indicator ----------------------------------------------------------

/// Draw the bottom home-indicator bar - a short, thin signal-colored
/// line centered horizontally at `y`. Every full-screen app / overlay
/// uses this as a passive "base of the screen" marker.
pub fn home_indicator<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    y: i32,
    tint: Rgb565,
) {
    let cx = theme::SCREEN_W as i32 / 2;
    Rectangle::new(
        Point::new(cx - HOME_INDICATOR_W / 2, y),
        Size::new(HOME_INDICATOR_W as u32, HOME_INDICATOR_H as u32),
    )
    .into_styled(PrimitiveStyle::with_fill(tint))
    .draw(display).ok();
}

// -- page_scrollbar ----------------------------------------------------------

/// Draw a vertical page-indicator scrollbar at the standard layout
/// position. Does nothing when `page_count < 2`.
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
        theme::SIGNAL,
        theme::SIGNAL_DIM,
    );
}

// -- Internal icon helpers for legacy header_bar -----------------------------

fn draw_close<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, cx: i32, cy: i32, half: i32, color: Rgb565,
) {
    let style = PrimitiveStyle::with_stroke(color, HEADER_ICON_STROKE);
    Line::new(
        Point::new(cx - half, cy - half),
        Point::new(cx + half, cy + half),
    ).into_styled(style).draw(display).ok();
    Line::new(
        Point::new(cx - half, cy + half),
        Point::new(cx + half, cy - half),
    ).into_styled(style).draw(display).ok();
}

fn draw_back<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, cx: i32, cy: i32, half: i32, color: Rgb565,
) {
    let style = PrimitiveStyle::with_stroke(color, HEADER_ICON_STROKE);
    Line::new(
        Point::new(cx + half, cy - half),
        Point::new(cx - half, cy),
    ).into_styled(style).draw(display).ok();
    Line::new(
        Point::new(cx - half, cy),
        Point::new(cx + half, cy + half),
    ).into_styled(style).draw(display).ok();
}
