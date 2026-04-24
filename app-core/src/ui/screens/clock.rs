//! Watch face - the Nightwatch OS ambient time display.
//!
//! Layout (top to bottom on a 410x502 canvas):
//! 1. Telemetry strip: cyan `SYS-ID <code>` on the left, chrome
//!    `DOW DD MON` on the right. Small mono-ish font.
//! 2. Swipe-hint bar: a thin cyan 2px line centered ~6px below the
//!    top, visible hit-zone for the swipe-down-from-top action. No
//!    per-screen handler; the Model routes that gesture.
//! 3. Stacked numerals: HH in signal red, MM directly below in bone.
//!    Both rendered in the geometric `Mega` (logisoso78) face, digits
//!    only. Meta row beneath: `:SS` in cyan + `LAT .. LON ..` in
//!    chrome.
//! 4. Two chamfered-outline tiles at the bottom, 6px gap:
//!    - left: signal-red border, mini heart + `062 BPM`
//!    - right: cyan border, mini envelope + `x03 UNREAD`
//!
//! Interactions:
//! - Tap anywhere in the hero band → open the app drawer.
//! - Tap on the UNREAD tile → (future) notifications screen. Currently
//!   a no-op; forwarded as a switch to Status as a placeholder so the
//!   tile reads as alive.
//!
//! Copy follows the spec's systemic voice: ALL CAPS chrome, leading
//! zeros on numerals, no em-dashes.

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
use u8g2_fonts::FontRenderer;

use crate::events::SystemEvent;
use crate::ui::{fonts, glyphs, theme};
use crate::ui::types::{Action, Screen, ScreenId, SystemData};
use crate::ui::widgets::{chamfered_panel, NOTCH};

// -- Geometry ---------------------------------------------------------------

const PAD_TOP: i32 = 28;
const PAD_X: i32 = 22;

/// Y of the swipe-hint bar (2px tall, 36px wide, centered on X).
const HINT_Y: i32 = 8;
const HINT_W: i32 = 36;
const HINT_H: i32 = 2;
/// Height of the top tap-zone that opens Quick Access. Matches the
/// spec's "36 px invisible hit zone" covering the visible cyan hint
/// bar and a bit of air above/below it. Full screen width - taps
/// don't have to land on the 36 px visible bar specifically.
const QA_TAP_H: i32 = 36;

/// Y of the telemetry strip's glyph tops.
const TELE_Y: i32 = PAD_TOP;

/// Bottom pill band.
const PILL_H: i32 = 38;
const PILL_BOTTOM_MARGIN: i32 = 36;
const PILL_GAP: i32 = 6;
const PILL_Y: i32 = theme::SCREEN_H as i32 - PILL_BOTTOM_MARGIN - PILL_H;

/// Hero HH/MM block - vertically centered between the telemetry strip
/// and the pill row with a slight upward bias so the meta row under
/// MM sits in the visual midline.
const HERO_HH_TOP: i32 = 120;
/// Spacing between HH baseline and MM baseline. Slight overlap
/// compared to a natural line-height so the two numerals read as one
/// stacked block.
const HERO_STACK_GAP: i32 = 72;
const HERO_MM_TOP: i32 = HERO_HH_TOP + HERO_STACK_GAP;
/// Y of the meta row (:SS + LAT/LON) under the MM glyphs.
const META_Y: i32 = HERO_MM_TOP + 84;

// -- Screen -----------------------------------------------------------------

pub struct ClockScreen;

impl ClockScreen {
    pub fn new() -> Self { Self }
}

impl Screen for ClockScreen {
    fn render<D: DrawTarget<Color = Rgb565>>(&self, display: &mut D, data: &SystemData) {
        draw_telemetry_strip(display, data);
        draw_swipe_hint(display);
        draw_hero_numerals(display, data);
        draw_meta_row(display, data);
        draw_bottom_tiles(display);
    }

    fn on_event(&mut self, event: &SystemEvent, _data: &mut SystemData) -> Action {
        match event {
            SystemEvent::PowerButtonLong => Action::Shutdown,
            // A seconds tick forces a redraw so `:SS` and the MM
            // digit keep in sync with the RTC.
            SystemEvent::TimeUpdated { .. } => Action::Redraw,
            // Dual tap zones per spec:
            // - Top 36 px (the swipe-hint band) → Quick Access
            // - Anywhere else → App Drawer
            // The 36 px top zone matches the visible HINT_W bar but
            // runs full width (`y < QA_TAP_H`), so the user doesn't
            // have to land on the thin cyan line precisely.
            SystemEvent::Tap { y, .. } => {
                if (*y as i32) < QA_TAP_H {
                    Action::SwitchScreen(ScreenId::QuickAccess)
                } else {
                    Action::SwitchScreen(ScreenId::AppDrawer)
                }
            }
            _ => Action::None,
        }
    }
}

// -- Draw helpers -----------------------------------------------------------

fn draw_swipe_hint<D: DrawTarget<Color = Rgb565>>(display: &mut D) {
    // 2px bar, 36px wide, centered horizontally. Cyan at 55% opacity
    // in the spec - we render at full saturation because embedded-
    // graphics has no blending and a dimmer cyan would just look
    // washed out on pure black.
    let cx = theme::SCREEN_W as i32 / 2;
    let x = cx - HINT_W / 2;
    Rectangle::new(
        Point::new(x, HINT_Y),
        Size::new(HINT_W as u32, HINT_H as u32),
    )
    .into_styled(PrimitiveStyle::with_fill(theme::CYAN))
    .draw(display).ok();
}

fn draw_telemetry_strip<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, data: &SystemData,
) {
    let font = fonts::caption();

    // Left: SYS-ID code. Static filler per the spec - no real
    // telemetry to report yet.
    fonts::draw_at(
        display, &font,
        "SYS-ID 232.29CB.98B",
        PAD_X, TELE_Y,
        theme::CYAN,
    );

    // Right: "TUE 24 APR".
    let weekday = crate::ui::screens::alarm::day_of_week(
        data.time.year as i32,
        data.time.month as i32,
        data.time.day as i32,
    );
    let dow = short_weekday(weekday);
    let mon = short_month(data.time.month);

    let mut buf: String<16> = String::new();
    let _ = write!(buf, "{} {:02} {}", dow, data.time.day, mon);

    fonts::draw_right(
        display, &font,
        buf.as_str(),
        theme::SCREEN_W as i32 - PAD_X, TELE_Y,
        theme::FG_MUTED,
    );
}

fn draw_hero_numerals<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, data: &SystemData,
) {
    let mega: FontRenderer = FontRenderer::new::<fonts::Mega>();
    let cx = theme::SCREEN_W as i32 / 2;

    let mut hh: String<4> = String::new();
    let _ = write!(hh, "{:02}", data.time.hour);
    fonts::draw_centered(display, &mega, hh.as_str(), cx, HERO_HH_TOP, theme::SIGNAL);

    let mut mm: String<4> = String::new();
    let _ = write!(mm, "{:02}", data.time.minute);
    fonts::draw_centered(display, &mega, mm.as_str(), cx, HERO_MM_TOP, theme::BONE);
}

fn draw_meta_row<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, data: &SystemData,
) {
    let font = fonts::caption();

    // Seconds block: ":SS" in cyan. The colon is part of the same
    // glyph run so spacing stays tight against the number.
    let mut ss: String<4> = String::new();
    let _ = write!(ss, ":{:02}", data.time.second);

    // Measure the seconds glyph width so we can place it and the
    // LAT/LON string side-by-side, separated by a fixed gap.
    let ss_w = fonts::measure_width(&font, ss.as_str());
    let coords = "LAT 0.8314  LON 2.6";
    let coords_w = fonts::measure_width(&font, coords);
    let gap = 14i32;
    let group_w = ss_w + gap + coords_w;

    let cx = theme::SCREEN_W as i32 / 2;
    let left_x = cx - group_w / 2;

    fonts::draw_at(display, &font, ss.as_str(),
        left_x, META_Y, theme::CYAN);
    fonts::draw_at(display, &font, coords,
        left_x + ss_w + gap, META_Y, theme::FG_MUTED);
}

fn draw_bottom_tiles<D: DrawTarget<Color = Rgb565>>(display: &mut D) {
    let total_w = theme::SCREEN_W as i32 - PAD_X * 2;
    let tile_w = (total_w - PILL_GAP) / 2;

    let left = Rectangle::new(
        Point::new(PAD_X, PILL_Y),
        Size::new(tile_w as u32, PILL_H as u32),
    );
    let right = Rectangle::new(
        Point::new(PAD_X + tile_w + PILL_GAP, PILL_Y),
        Size::new(tile_w as u32, PILL_H as u32),
    );

    // Smaller notch on pills so the chamfer reads at this height.
    let tile_notch = NOTCH - 2;

    // Left tile: heart glyph + 062 + BPM. Signal-red border, signal
    // text, chrome unit suffix.
    chamfered_panel(display, left, tile_notch, theme::SIGNAL, 1);
    draw_tile_contents(
        display, left,
        glyphs::heart, theme::SIGNAL,
        "062", "BPM",
        theme::SIGNAL,
    );

    // Right tile: envelope + x03 + UNREAD. Cyan border/text.
    chamfered_panel(display, right, tile_notch, theme::CYAN, 1);
    draw_tile_contents(
        display, right,
        glyphs::message, theme::CYAN,
        "x03", "UNREAD",
        theme::CYAN,
    );
}

/// Shared layout for the two bottom tiles: small glyph on the left,
/// mono-ish value in the center, uppercase suffix on the right. All
/// three vertically centered on the tile.
fn draw_tile_contents<D, F>(
    display: &mut D,
    rect: Rectangle,
    icon: F,
    icon_color: Rgb565,
    value: &str,
    suffix: &str,
    value_color: Rgb565,
)
where
    D: DrawTarget<Color = Rgb565>,
    F: FnOnce(&mut D, i32, i32, i32, Rgb565),
{
    let x = rect.top_left.x;
    let y = rect.top_left.y;
    let w = rect.size.width as i32;
    let h = rect.size.height as i32;
    let cy = y + h / 2;

    let pad = 14i32;
    let icon_cx = x + pad + 6;
    let icon_r = 7i32;

    icon(display, icon_cx, cy, icon_r, icon_color);

    let val_font = fonts::body();
    let suf_font = fonts::caption();
    let suf_w = fonts::measure_width(&suf_font, suffix);

    // Value sits flush-left after the icon column. Suffix sits flush-
    // right against the inner padding.
    fonts::draw_at(
        display, &val_font, value,
        x + pad + 22, cy - 8, value_color,
    );
    fonts::draw_right(
        display, &suf_font, suffix,
        x + w - pad, cy - 6, theme::FG_MUTED,
    );
    // Ignore suf_w - we intentionally right-align rather than
    // computing a joint x.
    let _ = suf_w;
}

// -- Small date helpers -----------------------------------------------------

fn short_weekday(w: u8) -> &'static str {
    match w % 7 {
        0 => "SUN",
        1 => "MON",
        2 => "TUE",
        3 => "WED",
        4 => "THU",
        5 => "FRI",
        _ => "SAT",
    }
}

fn short_month(m: u8) -> &'static str {
    match m {
        1 => "JAN",
        2 => "FEB",
        3 => "MAR",
        4 => "APR",
        5 => "MAY",
        6 => "JUN",
        7 => "JUL",
        8 => "AUG",
        9 => "SEP",
        10 => "OCT",
        11 => "NOV",
        _ => "DEC",
    }
}
