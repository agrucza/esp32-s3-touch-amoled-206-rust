//! Quick Access pull-down overlay.
//!
//! Reached by swiping down from the top edge (the Model routes that
//! gesture via `open_quick_access`). Contents are **visual only** for
//! now - no driver calls, no persistence. Tile state and brightness
//! live on the screen struct and reset to defaults every time the
//! overlay reopens.
//!
//! Layout (410x502 canvas):
//! - Top row: `QUICK.ACCESS` title in cyan + `^ PULL` telemetry hint.
//! - Brightness section: `BRIGHTNESS` label + current value + a
//!   clickable / draggable horizontal bar.
//! - Toggle grid: 4 across x 2 rows = 8 tiles
//!   (DND / AIRPLANE / FLASH / SAVER / BT / WIFI / SYNC / LOCK).
//!   Each tile is chamfered. Off = steel border + chrome caption.
//!   On = signal fill + signal border + black caption.
//! - Bottom: 2px signal-red home-indicator bar, centered.
//!
//! Interactions:
//! - Tap / drag on the brightness bar: compute the x-offset within
//!   the bar, clamp to [5, 100], store locally, redraw. Both taps
//!   and `TouchPressed` drags are honoured so scrubbing works.
//! - Tap on a toggle tile: flip the bool for that tile.
//! - Swipe up from anywhere: close overlay, return to the pre-overlay
//!   screen.

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{Point, Size},
    pixelcolor::Rgb565,
    prelude::Primitive,
    primitives::{PrimitiveStyle, Rectangle},
    Drawable,
};
use heapless::String;
use core::fmt::Write;

use crate::events::{SwipeDir, SystemEvent};
use crate::ui::{fonts, theme};
use crate::ui::types::{Action, Screen, ScreenId, SystemData};
use crate::ui::widgets::{chamfered_panel, NOTCH};

// -- Toggle tile metadata ----------------------------------------------------

#[derive(Clone, Copy)]
struct ToggleDef {
    label: &'static str,
}

const TOGGLES: [ToggleDef; 8] = [
    ToggleDef { label: "DND"      },
    ToggleDef { label: "AIR"      },
    ToggleDef { label: "FLASH"    },
    ToggleDef { label: "SAVER"    },
    ToggleDef { label: "BT"       },
    ToggleDef { label: "WIFI"     },
    ToggleDef { label: "SYNC"     },
    ToggleDef { label: "LOCK"     },
];

// -- Layout constants --------------------------------------------------------

const PAD_X: i32 = 22;
const HEADER_Y: i32 = 44;

/// Brightness bar block.
const BRIGHT_LABEL_Y: i32 = 90;
const BRIGHT_BAR_Y:   i32 = 116;
const BRIGHT_BAR_H:   i32 = 12;

/// Toggle grid: 4 tiles per row, 2 rows.
const TOGGLE_TOP: i32 = 170;
const TOGGLE_GAP: i32 = 6;

/// Bottom indicator bar.
const HOME_BAR_Y: i32 = theme::SCREEN_H as i32 - 18;
const HOME_BAR_W: i32 = 56;
const HOME_BAR_H: i32 = 2;

const GRID_BOTTOM: i32 = HOME_BAR_Y - 30;

fn bright_bar_rect() -> Rectangle {
    Rectangle::new(
        Point::new(PAD_X, BRIGHT_BAR_Y),
        Size::new(
            (theme::SCREEN_W as i32 - PAD_X * 2) as u32,
            BRIGHT_BAR_H as u32,
        ),
    )
}

fn toggle_rect(idx: usize) -> Rectangle {
    let row = idx / 4;
    let col = idx % 4;
    let total_w = theme::SCREEN_W as i32 - PAD_X * 2;
    let tile_w = (total_w - TOGGLE_GAP * 3) / 4;
    let total_h = GRID_BOTTOM - TOGGLE_TOP;
    let tile_h = (total_h - TOGGLE_GAP) / 2;

    let x = PAD_X + col as i32 * (tile_w + TOGGLE_GAP);
    let y = TOGGLE_TOP + row as i32 * (tile_h + TOGGLE_GAP);
    Rectangle::new(
        Point::new(x, y),
        Size::new(tile_w as u32, tile_h as u32),
    )
}

// -- Screen ------------------------------------------------------------------

pub struct QuickAccessScreen {
    /// Pre-overlay screen. The Model's nav stack already carries
    /// this entry, so the close path uses `Action::Back` to pop it.
    /// Field kept for future use (e.g. a "launched from X" hint).
    #[allow(dead_code)]
    previous: ScreenId,
    /// Local brightness (5..=100). Not persisted.
    brightness: u8,
    /// Per-tile on/off state.
    tiles_on: [bool; 8],
}

impl QuickAccessScreen {
    pub fn new(previous: ScreenId) -> Self {
        Self {
            previous,
            brightness: 68,
            tiles_on: [false; 8],
        }
    }

    /// Clamp an x-coordinate to the brightness bar and return the
    /// matching 5..=100 brightness value. Returns `None` if the point
    /// falls outside the bar's vertical range with slack.
    fn brightness_from_x(x: i32, y: i32) -> Option<u8> {
        let bar = bright_bar_rect();
        // Generous vertical tolerance so finger drags that wander
        // slightly above or below the bar still scrub.
        let vslop = 12i32;
        if y < bar.top_left.y - vslop
            || y >= bar.top_left.y + bar.size.height as i32 + vslop
        {
            return None;
        }
        let left = bar.top_left.x;
        let right = left + bar.size.width as i32;
        let clamped = x.clamp(left, right - 1);
        let frac = (clamped - left) as i32 * 95 / (bar.size.width as i32 - 1);
        Some((5 + frac).clamp(5, 100) as u8)
    }
}

impl Screen for QuickAccessScreen {
    fn render<D: DrawTarget<Color = Rgb565>>(&self, display: &mut D, _data: &SystemData) {
        // Header.
        let font_title = fonts::value();
        fonts::draw_at(
            display, &font_title,
            "QUICK.ACCESS",
            PAD_X, HEADER_Y - 8,
            theme::CYAN,
        );
        fonts::draw_right(
            display, &fonts::caption(),
            "^ PULL",
            theme::SCREEN_W as i32 - PAD_X, HEADER_Y,
            theme::FG_MUTED,
        );

        // Brightness label + current value.
        fonts::draw_at(
            display, &fonts::caption(),
            "BRIGHTNESS",
            PAD_X, BRIGHT_LABEL_Y,
            theme::CYAN,
        );
        let mut buf: String<8> = String::new();
        let _ = write!(buf, "{:02}%", self.brightness);
        fonts::draw_right(
            display, &fonts::caption(),
            buf.as_str(),
            theme::SCREEN_W as i32 - PAD_X, BRIGHT_LABEL_Y,
            theme::SIGNAL,
        );

        // Brightness bar: steel trough + signal fill to current %.
        let bar = bright_bar_rect();
        Rectangle::new(bar.top_left, bar.size)
            .into_styled(PrimitiveStyle::with_fill(theme::INK_3))
            .draw(display).ok();
        Rectangle::new(bar.top_left, bar.size)
            .into_styled(PrimitiveStyle::with_stroke(theme::STEEL, 1))
            .draw(display).ok();
        let fill_w = ((self.brightness as i32 - 5) * (bar.size.width as i32 - 2)) / 95;
        if fill_w > 0 {
            Rectangle::new(
                Point::new(bar.top_left.x + 1, bar.top_left.y + 1),
                Size::new(fill_w as u32, (bar.size.height - 2) as u32),
            )
            .into_styled(PrimitiveStyle::with_fill(theme::SIGNAL))
            .draw(display).ok();
        }

        // Toggle grid.
        for (i, t) in TOGGLES.iter().enumerate() {
            let rect = toggle_rect(i);
            let on = self.tiles_on[i];

            let border = if on { theme::SIGNAL } else { theme::STEEL };
            let label_color = if on { theme::BG } else { theme::FG_MUTED };

            if on {
                // Fill the interior (leaving 1 px clear for the chamfer outline).
                Rectangle::new(
                    Point::new(rect.top_left.x + 1, rect.top_left.y + 1),
                    Size::new(
                        (rect.size.width as i32 - 2) as u32,
                        (rect.size.height as i32 - 2) as u32,
                    ),
                )
                .into_styled(PrimitiveStyle::with_fill(theme::SIGNAL))
                .draw(display).ok();
            }

            let tile_notch = NOTCH - 4;
            chamfered_panel(display, rect, tile_notch, border, 1);

            fonts::draw_centered_in_rect(
                display, &fonts::caption(),
                t.label, rect, label_color,
            );
        }

        // Home indicator bar.
        let cx = theme::SCREEN_W as i32 / 2;
        Rectangle::new(
            Point::new(cx - HOME_BAR_W / 2, HOME_BAR_Y),
            Size::new(HOME_BAR_W as u32, HOME_BAR_H as u32),
        )
        .into_styled(PrimitiveStyle::with_fill(theme::SIGNAL))
        .draw(display).ok();
    }

    fn on_event(&mut self, event: &SystemEvent, _data: &mut SystemData) -> Action {
        match event {
            SystemEvent::PowerButtonLong => Action::Shutdown,

            // Swipe up from anywhere → close. `Action::Back` pops
            // the pre-overlay screen off the nav stack and switches
            // to it, so no orphan entry is left behind.
            SystemEvent::Swipe { dir: SwipeDir::Up, .. } => Action::Back,

            // Dragging on the brightness bar scrubs the value live.
            SystemEvent::TouchPressed { x, y } => {
                if let Some(v) = Self::brightness_from_x(*x as i32, *y as i32) {
                    if v != self.brightness {
                        self.brightness = v;
                        return Action::Redraw;
                    }
                }
                Action::None
            }

            // Bar taps (press + release in place) also snap.
            SystemEvent::Tap { x, y } => {
                let xi = *x as i32;
                let yi = *y as i32;
                if let Some(v) = Self::brightness_from_x(xi, yi) {
                    self.brightness = v;
                    return Action::Redraw;
                }
                let pt = Point::new(xi, yi);
                for i in 0..TOGGLES.len() {
                    if toggle_rect(i).contains(pt) {
                        self.tiles_on[i] = !self.tiles_on[i];
                        return Action::Redraw;
                    }
                }
                Action::None
            }

            _ => Action::None,
        }
    }
}
