//! Panel screen - the pull-down app picker.
//!
//! Implemented as a full `Screen` (not a manager-side overlay). The
//! manager only has to handle one thing: the swipe-down-from-header
//! gesture that opens it. Everything else - drawing, icon hit-testing,
//! page navigation, closing - is self-contained here.
//!
//! Layout:
//! - Full-screen amber background
//! - Upper dark rounded card containing the app picker (two icons per
//!   page, paginated via left/right swipes)
//! - Lower dark rounded pill as a placeholder "priority action"
//!
//! Gestures:
//! - Swipe up                   - close, return to previous screen
//! - Swipe left/right (content) - cycle carousel page (when > 1 page)
//! - Tap an icon                - switch to that app
//! - Tap the action pill        - no-op for now (reserved for future
//!   two-gesture confirmation)

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{Point, Size},
    pixelcolor::Rgb565,
    prelude::Primitive,
    primitives::{Circle, Line, PrimitiveStyle, Rectangle},
    Drawable,
};

use crate::events::{SwipeDir, SwipeRegion, SystemEvent};
use crate::ui::{fonts, primitives, theme};
use crate::ui::types::{Action, Screen, ScreenId, SystemData};

use super::HOME_APPS;

// -- Layout constants --------------------------------------------------------

const PANEL_H: i32 = theme::SCREEN_H as i32;
const PANEL_W: i32 = theme::SCREEN_W as i32;

/// Upper card - the app picker region. Full width, top half.
const CARD_X: i32 = 0;
const CARD_Y: i32 = 0;
const CARD_W: i32 = PANEL_W;
const CARD_H: i32 = PANEL_H / 2;
const CARD_RADIUS: u32 = 40;

// -- Icon geometry (derived from the card dimensions) ----------------------

/// Drawn circle radius (130 px diameter).
const ICON_RADIUS: i32 = 65;
/// Slightly larger tap target than the visible circle for easier hits.
const ICON_HIT_RADIUS: i32 = 71;
/// Pixel gap between the icon circle and the label below it.
const ICON_LABEL_GAP: i32 = 16;
/// Approximate height of the body font used for the label.
const ICON_LABEL_H: i32 = 14;

/// Vertical height of one icon-plus-label block.
const ICON_BLOCK_H: i32 = ICON_RADIUS * 2 + ICON_LABEL_GAP + ICON_LABEL_H;
/// Top y of the icon-plus-label block, vertically centered inside the card.
const ICON_BLOCK_TOP: i32 = CARD_Y + (CARD_H - ICON_BLOCK_H) / 2;
/// Vertical center of every icon circle (shared row).
const ICON_CY: i32 = ICON_BLOCK_TOP + ICON_RADIUS;
/// Top y of every label.
const ICON_LABEL_Y: i32 = ICON_CY + ICON_RADIUS + ICON_LABEL_GAP;

/// Number of app icons shown on one page. Fixed so the icons always
/// sit at the same size and visual position; more apps paginate
/// left/right rather than cramming into a single row.
const ICONS_PER_PAGE: usize = 2;

/// X center of the icon in column `col` (0 or 1 for the default
/// two-column layout). Each column gets an equal slice of the card
/// width, centered in its slice.
const fn icon_col_cx(col: usize) -> i32 {
    CARD_X + (CARD_W * (2 * col as i32 + 1)) / (2 * ICONS_PER_PAGE as i32)
}

#[inline]
const fn icon_col_position(col: usize) -> (i32, i32) {
    (icon_col_cx(col), ICON_CY)
}

/// Number of pages needed to show `app_count` apps at `ICONS_PER_PAGE`
/// icons per page. Rounds up so trailing apps get their own page.
const fn page_count(app_count: usize) -> usize {
    (app_count + ICONS_PER_PAGE - 1) / ICONS_PER_PAGE
}

/// Vertical center of the page-indicator dot row at the bottom of the card.
const PAGE_DOTS_CY: i32 = CARD_Y + CARD_H - 24;

// -- Lower action pill (derived from the region below the card) ------------

const LOWER_REGION_Y: i32 = CARD_Y + CARD_H;
const LOWER_REGION_H: i32 = PANEL_H - LOWER_REGION_Y;

const ACTION_PAD_X: i32 = 24;
/// Approximate visible glyph height of `fonts::value()` (helvB24).
/// Used to derive the pill height from the label size so they stay
/// proportional.
const ACTION_LABEL_H: i32 = 24;
/// Pill height = 5 × the label height. Makes the action pill a
/// real hero element that balances visually with the upper card.
const ACTION_H: i32 = ACTION_LABEL_H * 5;
const ACTION_X: i32 = CARD_X + ACTION_PAD_X;
const ACTION_W: i32 = CARD_W - ACTION_PAD_X * 2;
/// Pixels to shift the pill down from the geometric center of the
/// lower region. Positive moves it toward the bottom of the screen.
/// Stay at or below ~30 px to keep the pill's bottom corners clear of
/// the bezel.
const ACTION_Y_OFFSET: i32 = 0;
const ACTION_Y: i32 = LOWER_REGION_Y
    + (LOWER_REGION_H - ACTION_H) / 2
    + ACTION_Y_OFFSET;

const ACTION_LABEL: &str = "PLACEHOLDER";

// -- Screen implementation ---------------------------------------------------

pub struct PanelScreen {
    /// Current carousel page (0-indexed).
    page: usize,
    /// Screen the user came from, to return to on swipe-up.
    previous: ScreenId,
}

impl PanelScreen {
    /// Construct the panel with the screen it should return to on
    /// close. The initial page is the one containing `previous`.
    pub fn new(previous: ScreenId) -> Self {
        let idx = HOME_APPS
            .iter()
            .position(|s| *s == previous)
            .unwrap_or(0);
        Self {
            page: idx / ICONS_PER_PAGE,
            previous,
        }
    }
}

impl Screen for PanelScreen {
    fn render<D: DrawTarget<Color = Rgb565>>(&self, display: &mut D, _data: &SystemData) {
        // -- Full-screen amber background -----------------------------------
        Rectangle::new(
            Point::zero(),
            Size::new(theme::SCREEN_W as u32, theme::SCREEN_H as u32),
        )
        .into_styled(PrimitiveStyle::with_fill(theme::AMBER))
        .draw(display).ok();

        // -- Upper dark card: the app picker --------------------------------
        primitives::rounded_panel(
            display,
            CARD_X, CARD_Y, CARD_W, CARD_H,
            CARD_RADIUS,
            Some(theme::PANEL_BG),
            None,
        );

        // -- App icons for the current page ---------------------------------
        let pages = page_count(HOME_APPS.len());
        let start_idx = self.page * ICONS_PER_PAGE;
        for col in 0..ICONS_PER_PAGE {
            let app_idx = start_idx + col;
            if app_idx >= HOME_APPS.len() { break; }
            let app = HOME_APPS[app_idx];
            let is_active = app == self.previous;
            let (cx, cy) = icon_col_position(col);

            let border = if is_active { theme::AMBER } else { theme::AMBER_DIM };
            let glyph = if is_active { theme::TEXT_WHITE } else { theme::TEXT_DIM };
            let label_color = if is_active { theme::TEXT_WHITE } else { theme::TEXT_MUTED };

            primitives::circle_button(
                display,
                cx, cy, ICON_RADIUS,
                theme::BG,
                Some(border),
            );

            draw_app_icon(display, app, cx, cy, ICON_RADIUS - 12, glyph);

            fonts::draw_centered(
                display, &fonts::body(),
                app_display_name(app),
                cx, ICON_LABEL_Y,
                label_color,
            );
        }

        // -- Page indicator at the bottom of the card ----------------------
        if pages > 1 {
            primitives::dot_carousel(
                display,
                CARD_X + CARD_W / 2,
                PAGE_DOTS_CY,
                pages,
                self.page,
                theme::AMBER,
                theme::AMBER_DIM,
            );
        }

        // -- Lower dark action pill (placeholder) ---------------------------
        primitives::pill_solid(
            display,
            ACTION_X, ACTION_Y, ACTION_W, ACTION_H,
            theme::PANEL_BG,
        );

        // Glyph + label, centered as a group, both in amber so they
        // read as "cut out" of the dark pill and echo the background.
        let glyph_size = 20i32;
        let gap = 14i32;
        let label_w = fonts::measure_width(&fonts::value(), ACTION_LABEL);
        let group_w = glyph_size + gap + label_w;
        let group_x = ACTION_X + (ACTION_W - group_w) / 2;
        let pill_cy = ACTION_Y + ACTION_H / 2;

        Rectangle::new(
            Point::new(group_x, pill_cy - glyph_size / 2),
            Size::new(glyph_size as u32, glyph_size as u32),
        )
        .into_styled(PrimitiveStyle::with_fill(theme::AMBER))
        .draw(display).ok();

        let label_rect = Rectangle::new(
            Point::new(group_x + glyph_size + gap, ACTION_Y),
            Size::new(label_w as u32, ACTION_H as u32),
        );
        fonts::draw_centered_in_rect(
            display, &fonts::value(),
            ACTION_LABEL, label_rect,
            theme::AMBER,
        );
    }

    fn on_event(&mut self, event: &SystemEvent, _data: &SystemData) -> Action {
        match event {
            // Swipe up from anywhere: close the panel and go back.
            SystemEvent::Swipe { dir: SwipeDir::Up, .. } => {
                Action::SwitchScreen(self.previous)
            }
            // Left/right content swipes cycle the carousel page.
            SystemEvent::Swipe { dir: SwipeDir::Right, region: SwipeRegion::Content } => {
                let pages = page_count(HOME_APPS.len());
                if pages > 1 {
                    self.page = (self.page + 1) % pages;
                    Action::Redraw
                } else {
                    Action::None
                }
            }
            SystemEvent::Swipe { dir: SwipeDir::Left, region: SwipeRegion::Content } => {
                let pages = page_count(HOME_APPS.len());
                if pages > 1 {
                    self.page = (self.page + pages - 1) % pages;
                    Action::Redraw
                } else {
                    Action::None
                }
            }
            // Tap on an icon column resolves via current page to an
            // absolute app index and switches.
            SystemEvent::Tap { x, y } => {
                if let Some(col) = hit_icon_column(*x, *y) {
                    let app_idx = self.page * ICONS_PER_PAGE + col;
                    if let Some(&target) = HOME_APPS.get(app_idx) {
                        return Action::SwitchScreen(target);
                    }
                }
                // Future: hit_action(x, y) for the bottom pill.
                Action::None
            }
            SystemEvent::PowerButtonLong => Action::Shutdown,
            _ => Action::None,
        }
    }
}

// -- Hit tests (private helpers) --------------------------------------------

/// Returns the column (0..ICONS_PER_PAGE) containing (x, y), or None.
fn hit_icon_column(x: u16, y: u16) -> Option<usize> {
    let px = x as i32;
    let py = y as i32;
    for col in 0..ICONS_PER_PAGE {
        let (cx, cy) = icon_col_position(col);
        let dx = px - cx;
        let dy = py - cy;
        if dx * dx + dy * dy <= ICON_HIT_RADIUS * ICON_HIT_RADIUS {
            return Some(col);
        }
    }
    None
}

/// Reserved for the future "swipe + click" bottom-pill confirmation.
#[allow(dead_code)]
fn hit_action(x: u16, y: u16) -> bool {
    let px = x as i32;
    let py = y as i32;
    px >= ACTION_X
        && px < ACTION_X + ACTION_W
        && py >= ACTION_Y
        && py < ACTION_Y + ACTION_H
}

// -- App metadata & icon glyphs ---------------------------------------------

fn app_display_name(id: ScreenId) -> &'static str {
    match id {
        ScreenId::Clock => "Clock",
        ScreenId::Status => "Status",
        ScreenId::Panel => "Panel",
    }
}

fn draw_app_icon<D: DrawTarget<Color = Rgb565>>(
    display: &mut D,
    id: ScreenId,
    cx: i32, cy: i32,
    radius: i32,
    color: Rgb565,
) {
    match id {
        ScreenId::Clock => draw_clock_glyph(display, cx, cy, radius, color),
        ScreenId::Status => draw_status_glyph(display, cx, cy, radius, color),
        // Showing the panel inside the panel would be unusual but we
        // give it a sensible glyph anyway (four dots / grid).
        ScreenId::Panel => draw_panel_glyph(display, cx, cy, radius, color),
    }
}

fn draw_clock_glyph<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, cx: i32, cy: i32, radius: i32, color: Rgb565,
) {
    let thin = PrimitiveStyle::with_stroke(color, 2);
    let thick = PrimitiveStyle::with_stroke(color, 3);

    Circle::with_center(Point::new(cx, cy), (radius * 2) as u32)
        .into_styled(thin).draw(display).ok();

    Line::new(
        Point::new(cx, cy),
        Point::new(cx, cy - radius * 2 / 3),
    ).into_styled(thick).draw(display).ok();

    Line::new(
        Point::new(cx, cy),
        Point::new(cx + radius / 3, cy - radius / 3),
    ).into_styled(thick).draw(display).ok();

    Circle::with_center(Point::new(cx, cy), 4)
        .into_styled(PrimitiveStyle::with_fill(color))
        .draw(display).ok();
}

fn draw_status_glyph<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, cx: i32, cy: i32, radius: i32, color: Rgb565,
) {
    let bar_w = (radius / 3).max(4);
    let gap = 4;
    let total_w = 3 * bar_w + 2 * gap;
    let start_x = cx - total_w / 2;
    let base_y = cy + radius / 2;
    let heights = [radius / 2, radius * 3 / 4, radius];
    let fill = PrimitiveStyle::with_fill(color);

    for (i, h) in heights.iter().enumerate() {
        let x = start_x + i as i32 * (bar_w + gap);
        let y = base_y - *h;
        Rectangle::new(Point::new(x, y), Size::new(bar_w as u32, *h as u32))
            .into_styled(fill).draw(display).ok();
    }
}

fn draw_panel_glyph<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, cx: i32, cy: i32, radius: i32, color: Rgb565,
) {
    // Four small filled squares in a 2x2 grid.
    let s = (radius / 3).max(4);
    let gap = 4;
    let offsets = [
        (-(s + gap / 2), -(s + gap / 2)),
        ( gap / 2,       -(s + gap / 2)),
        (-(s + gap / 2),  gap / 2),
        ( gap / 2,        gap / 2),
    ];
    let fill = PrimitiveStyle::with_fill(color);
    for (dx, dy) in offsets.iter() {
        Rectangle::new(
            Point::new(cx + dx, cy + dy),
            Size::new(s as u32, s as u32),
        )
        .into_styled(fill).draw(display).ok();
    }
}
