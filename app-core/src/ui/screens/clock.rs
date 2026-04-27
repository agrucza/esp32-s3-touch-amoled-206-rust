//! Watch face - the Nightwatch OS ambient time display.
//!
//! Layout (top to bottom on a 410x502 canvas):
//! 1. Telemetry strip: cyan `SYS-ID <code>` on the left, chrome
//!    `DOW DD MON` on the right. Small mono-ish font.
//! 2. Swipe-hint bar: a thin cyan 2 px line near the top, a visual
//!    cue for the swipe-down-from-top edge gesture (routed by the
//!    Model into Quick Access).
//! 3. Stacked numerals: HH in signal red, MM directly below in bone.
//!    Both rendered in the geometric `Mega` (logisoso78) face, digits
//!    only. Meta row beneath: `:SS` in cyan + `LAT .. LON ..` in
//!    chrome.
//! 4. Two chamfered info tiles at the bottom (via `info_tile` +
//!    `layout::bottom_tile_row::<2>()`):
//!    - left: yellow border, bell glyph, next enabled alarm time
//!      (`HH:MM`) or `OFF` if none, suffix `ALARM`.
//!    - right: orange border, hourglass glyph, timer remaining
//!      (`MM:SS` < 1 h, `HH:MM` ≥ 1 h) or `OFF`, suffix `TIMER`.
//!
//! Interactions:
//! - Tap on the alarm tile → switch to Alarm screen.
//! - Tap on the timer tile → switch to Timer screen.
//! - Anywhere else: no-op. Quick Access reaches via swipe-down-from-
//!   top, App Drawer via swipe-up-from-bottom (both at the Model
//!   level, not handled here).
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
use crate::ui::{fonts, glyphs, layout, theme};
use crate::ui::types::{Action, Screen, ScreenId, SystemData};
use crate::ui::widgets::info_tile;

// -- Geometry ---------------------------------------------------------------

const PAD_TOP: i32 = 28;
const PAD_X: i32 = 40;

/// Y of the swipe-hint bar (2px tall, 36px wide, centered on X).
const HINT_Y: i32 = 8;
const HINT_W: i32 = 36;
const HINT_H: i32 = 2;

/// Y of the telemetry strip's glyph tops.
const TELE_Y: i32 = PAD_TOP;

/// Hero HH/MM block - vertically centered between the telemetry strip
/// and the bottom tile row with a slight upward bias so the meta row
/// under MM sits in the visual midline.
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
        draw_bottom_tiles(display, data);
    }

    fn on_event(&mut self, event: &SystemEvent, _data: &mut SystemData) -> Action {
        match event {
            SystemEvent::PowerButtonLong => Action::Shutdown,
            // A seconds tick forces a redraw so `:SS` and the MM
            // digit keep in sync with the RTC.
            SystemEvent::TimeUpdated { .. } => Action::Redraw,
            // Discrete state transitions that happen between seconds
            // and would otherwise leave the bottom tiles showing
            // stale data until the next TimeUpdated:
            // - AlarmFired: an enabled alarm just rang. The Model
            //   may snooze it / disable it / advance the next-alarm
            //   pointer, so re-render the alarm tile.
            // - TimerExpired: the running timer hit zero and reset
            //   to Idle, so the timer tile should flip to OFF.
            SystemEvent::AlarmFired | SystemEvent::TimerExpired => Action::Redraw,
            // Bottom tiles route to their target apps; everywhere else
            // is a no-op. Quick Access opens via swipe-down-from-top
            // and App Drawer via swipe-up-from-bottom (both routed at
            // the Model level, not here).
            SystemEvent::Tap { x, y } => {
                let [left, right] = layout::bottom_tile_row::<2>();
                if rect_hit(left, *x, *y) {
                    Action::SwitchScreen(ScreenId::Alarm)
                } else if rect_hit(right, *x, *y) {
                    Action::SwitchScreen(ScreenId::Timer)
                } else {
                    Action::None
                }
            }
            _ => Action::None,
        }
    }
}

fn rect_hit(rect: Rectangle, x: u16, y: u16) -> bool {
    let px = x as i32;
    let py = y as i32;
    let rx = rect.top_left.x;
    let ry = rect.top_left.y;
    px >= rx
        && px < rx + rect.size.width as i32
        && py >= ry
        && py < ry + rect.size.height as i32
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

fn draw_bottom_tiles<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, data: &SystemData,
) {
    let [left, right] = layout::bottom_tile_row::<2>();

    // -- Alarm tile: next enabled alarm time, or OFF -----------------------
    let weekday = crate::ui::screens::alarm::day_of_week(
        data.time.year as i32,
        data.time.month as i32,
        data.time.day as i32,
    );
    let mut alarm_buf: String<8> = String::new();
    let alarm_value: &str = match data.alarms.next_alarm(
        data.time.hour, data.time.minute, weekday,
    ) {
        Some(idx) => {
            let entry = &data.alarms.entries[idx];
            let _ = write!(alarm_buf, "{:02}:{:02}", entry.hour, entry.minute);
            alarm_buf.as_str()
        }
        None => "OFF",
    };
    info_tile(display, left, glyphs::bell, alarm_value, "ALARM", theme::YELLOW);

    // -- Timer tile: remaining time, or OFF when idle/zero -----------------
    let mut timer_buf: String<8> = String::new();
    let secs = data.timer.remaining().as_secs();
    let timer_value: &str = if secs == 0 {
        "OFF"
    } else if secs < 3600 {
        let _ = write!(timer_buf, "{:02}:{:02}", secs / 60, secs % 60);
        timer_buf.as_str()
    } else {
        let _ = write!(timer_buf, "{:02}:{:02}", secs / 3600, (secs / 60) % 60);
        timer_buf.as_str()
    };
    info_tile(display, right, glyphs::hourglass, timer_value, "TIMER", theme::ORANGE);
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
