//! Quick Access pull-down overlay.
//!
//! Reached by swiping down from the top edge (the Model routes that
//! gesture via `open_quick_access`). Contents are **visual only** for
//! now - no driver calls, no persistence. Tile state and brightness
//! live on the screen struct and reset to defaults every time the
//! overlay reopens.
//!
//! Layout (410x502 canvas):
//! - Top row: `QUICK.ACCESS` title in cyan + `↓ PULL` telemetry hint.
//! - Brightness section: `BRIGHTNESS` label + current value + a
//!   clickable / draggable horizontal bar.
//! - Toggle grid: 4 across x 2 rows = 8 tiles
//!   (DND / AIR / FLASH / SAVER / BT / WIFI / SYNC / LOCK), each with
//!   an icon over its label. Off = steel border + chrome caption.
//!   On = signal fill + signal border + black icon/caption.
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
use crate::ui::{fonts, glyphs, theme};
use crate::ui::types::{Action, Screen, ScreenId, SystemData};
use crate::ui::widgets::{
    chamfered_panel, home_indicator, status_bar, NOTCH, STATUS_BAR_H,
};

// -- Toggle tile metadata ----------------------------------------------------

/// Icon kind for a toggle tile. Enum-dispatched to the concrete glyph
/// at render time (same pattern the App Drawer uses).
#[derive(Clone, Copy)]
enum TileIcon {
    Dnd,      // moon (close-enough placeholder for do-not-disturb)
    Airplane, // play triangle (placeholder; no dedicated airplane glyph yet)
    Flash,    // lightning bolt
    Saver,    // battery
    Bluetooth,
    Wifi,     // signal bars (close-enough placeholder)
    Sync,     // stopwatch dial (placeholder)
    Lock,
}

fn draw_tile_icon<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, kind: TileIcon, cx: i32, cy: i32, color: Rgb565,
) {
    let r = 9;
    match kind {
        TileIcon::Dnd       => glyphs::moon(display, cx, cy, r, color),
        TileIcon::Airplane  => glyphs::play(display, cx, cy, r, color),
        TileIcon::Flash     => glyphs::bolt(display, cx, cy, r, color),
        TileIcon::Saver     => glyphs::battery(display, cx, cy, r, color),
        TileIcon::Bluetooth => glyphs::bluetooth_small(display, cx, cy, r, color),
        TileIcon::Wifi      => glyphs::signal_small(display, cx, cy, r, color),
        TileIcon::Sync      => glyphs::stopwatch(display, cx, cy, r, color),
        TileIcon::Lock      => glyphs::lock(display, cx, cy, r, color),
    }
}

#[derive(Clone, Copy)]
struct ToggleDef {
    label: &'static str,
    icon: TileIcon,
}

const TOGGLES: [ToggleDef; 8] = [
    ToggleDef { label: "DND",   icon: TileIcon::Dnd       },
    ToggleDef { label: "AIR",   icon: TileIcon::Airplane  },
    ToggleDef { label: "FLASH", icon: TileIcon::Flash     },
    ToggleDef { label: "SAVER", icon: TileIcon::Saver     },
    ToggleDef { label: "BT",    icon: TileIcon::Bluetooth },
    ToggleDef { label: "WIFI",  icon: TileIcon::Wifi      },
    ToggleDef { label: "SYNC",  icon: TileIcon::Sync      },
    ToggleDef { label: "LOCK",  icon: TileIcon::Lock      },
];

// -- Layout constants --------------------------------------------------------

const PAD_X: i32 = 22;

/// Y of the top status bar.
const STATUS_Y: i32 = 0;
/// Horizontal inset for status-bar content around the bezel arc.
const STATUS_X_INSET: i32 = 85;

const HEADER_Y: i32 = STATUS_Y + STATUS_BAR_H + 26;

/// Brightness bar block.
const BRIGHT_LABEL_Y: i32 = HEADER_Y + 46;
const BRIGHT_BAR_Y:   i32 = BRIGHT_LABEL_Y + 26;
const BRIGHT_BAR_H:   i32 = 12;

/// Toggle grid: 4 tiles per row, 2 rows.
const TOGGLE_TOP: i32 = BRIGHT_BAR_Y + 54;
const TOGGLE_GAP: i32 = 6;

/// Bottom indicator bar.
const HOME_BAR_Y: i32 = theme::SCREEN_H as i32 - 18;

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
    /// Per-tile on/off state. Purely in-session - the overlay is
    /// ephemeral and the tiles don't back real prefs yet.
    tiles_on: [bool; 8],
}

impl QuickAccessScreen {
    pub fn new(previous: ScreenId) -> Self {
        Self {
            previous,
            tiles_on: [false; 8],
        }
    }

    /// Clamp an x-coordinate to the brightness bar and return the
    /// matching brightness percent, using the slider's current max
    /// (5..100 normally, 5..30 when night_mode is on). `None` if the
    /// point falls outside the bar's vertical range with slack.
    fn brightness_from_x(x: i32, y: i32, max_pct: u8) -> Option<u8> {
        let bar = bright_bar_rect();
        let vslop = 12i32;
        if y < bar.top_left.y - vslop
            || y >= bar.top_left.y + bar.size.height as i32 + vslop
        {
            return None;
        }
        let left = bar.top_left.x;
        let right = left + bar.size.width as i32;
        let clamped = x.clamp(left, right - 1);
        let range = (max_pct as i32 - 5).max(1);
        let frac = (clamped - left) as i32 * range / (bar.size.width as i32 - 1);
        Some((5 + frac).clamp(5, max_pct as i32) as u8)
    }

    /// Current brightness percent as read from the live config on
    /// `data`. Converts the hardware 0..=255 back into the 5..=100
    /// slider range.
    fn brightness_pct(data: &SystemData) -> u8 {
        let hw = data.config.display.brightness_active as u16;
        ((hw * 100 / 255) as u8).clamp(5, 100)
    }
}

impl Screen for QuickAccessScreen {
    fn render<D: DrawTarget<Color = Rgb565>>(&self, display: &mut D, data: &SystemData) {
        // Top status bar with cyan tint per the spec (Quick Access is
        // a cyan-accent overlay).
        let mut time_buf: heapless::String<8> = heapless::String::new();
        let _ = core::fmt::Write::write_fmt(
            &mut time_buf,
            format_args!("{:02}:{:02}", data.time.hour, data.time.minute),
        );
        status_bar(
            display,
            STATUS_Y,
            time_buf.as_str(),
            data.power.battery_percent,
            theme::CYAN,
            STATUS_X_INSET,
        );
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
            "v PULL",
            theme::SCREEN_W as i32 - PAD_X, HEADER_Y,
            theme::FG_MUTED,
        );

        // Brightness label + current value - computed once from
        // `data.config`, used for both the "NN%" readout and the
        // bar's fill width. No cached state on the screen.
        let brightness = Self::brightness_pct(data);
        fonts::draw_at(
            display, &fonts::caption(),
            "BRIGHTNESS",
            PAD_X, BRIGHT_LABEL_Y,
            theme::CYAN,
        );
        let mut buf: String<8> = String::new();
        let _ = write!(buf, "{:02}%", brightness);
        fonts::draw_right(
            display, &fonts::caption(),
            buf.as_str(),
            theme::SCREEN_W as i32 - PAD_X, BRIGHT_LABEL_Y,
            theme::SIGNAL,
        );

        // Brightness bar: steel trough + signal fill to current %.
        // The bar's full width represents the slider's current range
        // (5..100 normally, 5..30 when night_mode is on), so a
        // value at the top of the range fills the bar completely
        // regardless of night_mode.
        let bar = bright_bar_rect();
        Rectangle::new(bar.top_left, bar.size)
            .into_styled(PrimitiveStyle::with_fill(theme::INK_3))
            .draw(display).ok();
        Rectangle::new(bar.top_left, bar.size)
            .into_styled(PrimitiveStyle::with_stroke(theme::STEEL, 1))
            .draw(display).ok();
        let max_pct = data.config.display.max_brightness_pct() as i32;
        let range = (max_pct - 5).max(1);
        let fill_w = ((brightness as i32 - 5).max(0) * (bar.size.width as i32 - 2)) / range;
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

            // Off: chrome label + chrome icon + steel border, no fill.
            // On: black label + black icon on a signal fill, signal border.
            let border = if on { theme::SIGNAL } else { theme::STEEL };
            let content_color = if on { theme::BG } else { theme::FG_MUTED };

            if on {
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

            // Icon in the upper half, label in the lower half.
            let h = rect.size.height as i32;
            let icon_cx = rect.top_left.x + rect.size.width as i32 / 2;
            let icon_cy = rect.top_left.y + h * 38 / 100;
            draw_tile_icon(display, t.icon, icon_cx, icon_cy, content_color);

            let label_rect = Rectangle::new(
                Point::new(rect.top_left.x, rect.top_left.y + h * 60 / 100),
                Size::new(rect.size.width, (h * 40 / 100) as u32),
            );
            fonts::draw_centered_in_rect(
                display, &fonts::caption(),
                t.label, label_rect, content_color,
            );
        }

        home_indicator(display, HOME_BAR_Y, theme::SIGNAL);
    }

    fn on_event(&mut self, event: &SystemEvent, data: &mut SystemData) -> Action {
        match event {
            SystemEvent::PowerButtonLong => Action::Shutdown,

            // Swipe up from anywhere → close. `Action::Back` pops
            // the pre-overlay screen off the nav stack and switches
            // to it, so no orphan entry is left behind.
            SystemEvent::Swipe { dir: SwipeDir::Up, .. } => Action::Back,

            // Dragging on the brightness bar scrubs the value live.
            // Each meaningful delta fires a SetBrightness action so the
            // Model can update config + hardware. The screen itself
            // keeps no brightness state - next render reads back from
            // `data.config`.
            SystemEvent::TouchPressed { x, y } => {
                let max_pct = data.config.display.max_brightness_pct();
                if let Some(v) = Self::brightness_from_x(*x as i32, *y as i32, max_pct) {
                    if v != Self::brightness_pct(data) {
                        return Action::SetBrightness { percent: v };
                    }
                }
                Action::None
            }

            // Tap handling: only toggle tiles here. Bar taps are
            // already handled by the TouchPressed -> TouchReleased
            // cycle above (TouchPressed applies via SetBrightness,
            // TouchReleased flushes SaveConfig). Re-handling here
            // would re-dirty the config with no following release
            // to flush it.
            SystemEvent::Tap { x, y } => {
                let pt = Point::new(*x as i32, *y as i32);
                let max_pct = data.config.display.max_brightness_pct();
                // Ignore taps that land on the brightness bar.
                if Self::brightness_from_x(*x as i32, *y as i32, max_pct).is_some() {
                    return Action::None;
                }
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
