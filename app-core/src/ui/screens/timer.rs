//! Timer screen - count-down timer rebuilt on the Nightwatch theme.
//!
//! Two internal views:
//!
//! **Main view**:
//! - Standard app chrome: status bar (orange-tinted), Nightwatch
//!   header `TIMER` + `TMR.0001` system-code, signal-red home
//!   indicator at the bottom.
//! - Readout panel: orange border, `REMAINING` tag-label TL,
//!   hero-font `HH:MM:SS` centered. Tappable when not running to
//!   open the numpad. Border + numerals + tag flash orange↔signal
//!   during the post-expiry alert.
//! - Action row: START / PAUSE / RESUME (Primary orange) and
//!   RESET (Ghost steel when zero, Primary signal-red when there's
//!   a duration set).
//!
//! **Numpad view** - duration entry:
//! - Standard app chrome with chevron-back returning to Main.
//! - Time label below the header showing entered `HH:MM:SS`,
//!   right-to-left fill like a calculator.
//! - Existing `Numpad` widget below. The numpad still uses
//!   `theme::SIGNAL` for its keys; tinting it to follow the
//!   per-screen accent is a future change once we have a second
//!   accent caller.
//!
//! Tapping the readout in idle/paused opens the numpad. Confirming
//! on the numpad sets the duration and returns to Main. If the
//! entered duration exceeds the hardware countdown maximum (4h15m),
//! it's clamped and the time label flashes orange↔red twice. The
//! user must confirm again with the capped value.

use core::fmt::Write;

use embassy_time::{Duration, Instant};
use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{Point, Size},
    pixelcolor::Rgb565,
    primitives::Rectangle,
};

use crate::events::SystemEvent;
use crate::ui::{fonts, layout, theme};
use crate::ui::types::{Action, Screen, SystemData, TimerState};
use crate::data::TimeData;
use crate::ui::widgets::{
    app_chrome_back_hit, chamfered_button, chamfered_panel, draw_app_chrome,
    tag_label, ButtonVariant, Numpad, NumpadAction,
    APP_CONTENT_TOP, NOTCH, TAG_LABEL_H,
};

// -- Constants ---------------------------------------------------------------

/// Per-screen accent. Orange = "data stream / dynamic" per the spec;
/// also differentiates Timer (counting down) from Stopwatch (green,
/// counting up).
const ACCENT: Rgb565 = theme::ORANGE;

/// Static system-code shown in the header's right-telemetry slot.
const TELEMETRY: &str = "TMR.0001";

/// Side margin matching the readout panel in stopwatch / settings.
const SIDE_MARGIN: i32 = layout::VSTACK_SIDE_MARGIN;

/// Readout panel height.
const READOUT_H: i32 = 130;

/// Gap between the readout's bottom edge and the action row.
const READOUT_BUTTON_GAP: i32 = 8;

/// Y of the readout panel's top edge - vertically centred between
/// the header bottom and the action row.
const READOUT_TOP: i32 = APP_CONTENT_TOP
    + (layout::BOTTOM_TILE_Y - READOUT_BUTTON_GAP - APP_CONTENT_TOP - READOUT_H) / 2;

/// Y of the time label (top of glyphs) in the numpad view. Sits
/// below the header with breathing room before the numpad grid.
const NUMPAD_TIME_Y: i32 = APP_CONTENT_TOP + 28;

/// Maximum timer duration in seconds (255 * 60s = 4h15m), capped
/// by the PCF85063 hardware countdown register.
const MAX_TIMER_SECS: u64 = 15300;

/// Ticks per flash phase (250 ms at 20 Hz = 5 ticks).
const FLASH_PHASE_TICKS: u8 = 5;

/// Total flash ticks for the clamp-warning animation (4 phases = 1 s).
const FLASH_TOTAL_TICKS: u8 = FLASH_PHASE_TICKS * 4;

// -- Internal types ----------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimerView {
    Main,
    Numpad,
}

// -- Screen ------------------------------------------------------------------

pub struct TimerScreen {
    view: TimerView,
    /// Last displayed remaining second, gates 1 Hz redraw.
    last_rendered_sec: u64,
    /// Numpad widget for duration entry.
    numpad: Numpad,
    /// Remaining flash ticks for the clamp-warning animation.
    /// Alternates the numpad time label between accent and danger.
    flash_ticks: u8,
    /// True when the timer has expired and we're alerting the user.
    /// Any tap dismisses the alert and stops the buzz.
    alerting: bool,
    /// Tick counter for the alert flash, incremented per
    /// `MotionUpdated` while alerting.
    alert_ticks: u8,
}

impl TimerScreen {
    pub fn new() -> Self {
        Self {
            view: TimerView::Main,
            last_rendered_sec: 0,
            numpad: Numpad::new(6),
            flash_ticks: 0,
            alerting: false,
            alert_ticks: 0,
        }
    }

    /// Compute remaining seconds from current RTC wall time + the
    /// stored target seconds-since-midnight. Handles midnight wrap.
    fn remaining_from_rtc(target_secs: u32, time: &TimeData) -> u32 {
        let now_secs = time.hour as u32 * 3600
            + time.minute as u32 * 60
            + time.second as u32;
        if target_secs >= now_secs {
            target_secs - now_secs
        } else {
            (24 * 3600 - now_secs) + target_secs
        }
    }

    /// Set up the running state: compute target_secs from current
    /// RTC time and arm the embassy deadline.
    fn start_countdown(secs: u32, data: &mut SystemData) {
        let now_secs = data.time.hour as u32 * 3600
            + data.time.minute as u32 * 60
            + data.time.second as u32;
        let target_secs = (now_secs + secs) % (24 * 3600);
        data.timer = TimerState::Running {
            deadline: Instant::now() + Duration::from_secs(secs as u64),
            target_secs,
        };
    }

    /// Color the readout panel + numerals while alerting, flashing
    /// at 250 ms per phase between the screen accent and danger.
    fn alert_color(&self) -> Rgb565 {
        let phase = self.alert_ticks / FLASH_PHASE_TICKS;
        if phase % 2 == 0 { ACCENT } else { theme::DANGER }
    }

    /// Color the numpad time label, flashing between accent and
    /// danger during the post-clamp warning animation.
    fn time_label_color(&self) -> Rgb565 {
        if self.flash_ticks == 0 {
            return ACCENT;
        }
        let phase = (FLASH_TOTAL_TICKS - self.flash_ticks) / FLASH_PHASE_TICKS;
        if phase % 2 == 0 { theme::DANGER } else { ACCENT }
    }
}

impl Screen for TimerScreen {
    fn render<D: DrawTarget<Color = Rgb565>>(&self, display: &mut D, data: &SystemData) {
        match self.view {
            TimerView::Main => self.render_main(display, data),
            TimerView::Numpad => self.render_numpad(display, data),
        }
    }

    fn on_event(&mut self, event: &SystemEvent, data: &mut SystemData) -> Action {
        if matches!(event, SystemEvent::PowerButtonLong) {
            return Action::Shutdown;
        }

        // RTC hardware timer expired - start alerting.
        if matches!(event, SystemEvent::TimerExpired) {
            self.alerting = true;
            self.view = TimerView::Main;
            return Action::StartBuzz { on_ms: 200, off_ms: 100 };
        }

        // While alerting: tick the flash, dismiss on tap.
        if self.alerting {
            if matches!(event, SystemEvent::Tap { .. }) {
                self.alerting = false;
                self.alert_ticks = 0;
                return Action::StopBuzz;
            }
            if matches!(event, SystemEvent::MotionUpdated { .. }) {
                let old_phase = self.alert_ticks / FLASH_PHASE_TICKS;
                self.alert_ticks = self.alert_ticks.wrapping_add(1);
                let new_phase = self.alert_ticks / FLASH_PHASE_TICKS;
                if new_phase != old_phase {
                    return Action::Redraw;
                }
            }
            return Action::None;
        }

        // Resync embassy deadline from RTC time on every wall-clock
        // tick. Without this the embassy Instant drifts across light
        // sleep / RTC adjustment.
        if let SystemEvent::TimeUpdated { data: time } = event {
            if let TimerState::Running { target_secs, .. } = data.timer {
                let remaining = Self::remaining_from_rtc(target_secs, time);
                if remaining == 0 {
                    data.timer = TimerState::Idle {
                        duration: Duration::from_ticks(0),
                    };
                } else {
                    data.timer = TimerState::Running {
                        deadline: Instant::now() + Duration::from_secs(remaining as u64),
                        target_secs,
                    };
                }
                return Action::Redraw;
            }
        }

        match self.view {
            TimerView::Main => self.main_event(event, data),
            TimerView::Numpad => self.numpad_event(event, data),
        }
    }
}

// -- Main view ---------------------------------------------------------------

impl TimerScreen {
    fn render_main<D: DrawTarget<Color = Rgb565>>(&self, display: &mut D, data: &SystemData) {
        draw_app_chrome(display, data, "TIMER", TELEMETRY, ACCENT);

        // -- Readout panel -------------------------------------------------
        let panel_color = if self.alerting { self.alert_color() } else { ACCENT };
        let panel = readout_rect();
        chamfered_panel(display, panel, NOTCH, panel_color, 1);
        let tag_text = if self.alerting { "EXPIRED" } else { "REMAINING" };
        tag_label(
            display,
            panel.top_left.x,
            panel.top_left.y,
            tag_text,
            panel_color,
            NOTCH,
        );

        let mut buf: heapless::String<12> = heapless::String::new();
        format_duration(data.timer.remaining(), &mut buf);

        let inner_rect = Rectangle::new(
            Point::new(
                panel.top_left.x,
                panel.top_left.y + TAG_LABEL_H,
            ),
            Size::new(
                panel.size.width,
                panel.size.height - TAG_LABEL_H as u32,
            ),
        );
        fonts::draw_centered_in_rect(
            display, &fonts::hero(),
            buf.as_str(), inner_rect, panel_color,
        );

        // -- Action row ----------------------------------------------------
        let [left, right] = layout::bottom_tile_row::<2>();

        let run_label = match data.timer {
            TimerState::Running { .. } => "PAUSE",
            TimerState::Paused { remaining } if remaining.as_ticks() > 0 => "RESUME",
            _ => "START",
        };
        // START is meaningful only when there's a duration to run.
        // For Idle@zero we still draw Primary so the affordance is
        // visible; the tap is rejected by `main_event`.
        chamfered_button(display, left, run_label, ButtonVariant::Primary, ACCENT);

        if data.timer.remaining().as_secs() == 0 {
            chamfered_button(
                display, right, "RESET",
                ButtonVariant::Ghost, theme::STEEL,
            );
        } else {
            chamfered_button(
                display, right, "RESET",
                ButtonVariant::Primary, theme::SIGNAL,
            );
        }
    }

    fn main_event(&mut self, event: &SystemEvent, data: &mut SystemData) -> Action {
        match event {
            // 20 Hz tick: redraw only when the displayed second changes.
            SystemEvent::MotionUpdated { .. }
                if data.timer.is_running() =>
            {
                let sec = data.timer.remaining().as_secs();
                if sec != self.last_rendered_sec {
                    self.last_rendered_sec = sec;
                    Action::Redraw
                } else {
                    Action::None
                }
            }

            // Header back chevron: pop the nav stack.
            SystemEvent::Tap { x, y } if app_chrome_back_hit(*x, *y) => {
                Action::Back
            }

            // Tap the readout panel (when not running) → numpad.
            SystemEvent::Tap { x, y }
                if !data.timer.is_running() && rect_hit(readout_rect(), *x, *y) =>
            {
                self.numpad.prefill(&duration_to_raw(data.timer.remaining()));
                self.view = TimerView::Numpad;
                Action::Redraw
            }

            SystemEvent::Tap { x, y } => {
                let [left, right] = layout::bottom_tile_row::<2>();
                if rect_hit(left, *x, *y) {
                    match data.timer {
                        TimerState::Idle { duration } if duration.as_ticks() > 0 => {
                            let secs = duration.as_secs() as u32;
                            Self::start_countdown(secs, data);
                            Action::StartTimer { seconds: secs }
                        }
                        TimerState::Running { deadline, .. } => {
                            let now = Instant::now();
                            let remaining = if now >= deadline {
                                Duration::from_ticks(0)
                            } else {
                                deadline.duration_since(now)
                            };
                            data.timer = TimerState::Paused { remaining };
                            Action::CancelTimer
                        }
                        TimerState::Paused { remaining } if remaining.as_ticks() > 0 => {
                            let secs = remaining.as_secs() as u32;
                            Self::start_countdown(secs, data);
                            Action::StartTimer { seconds: secs }
                        }
                        // Idle@zero or Paused@zero - nothing to start.
                        _ => Action::None,
                    }
                } else if rect_hit(right, *x, *y) {
                    if data.timer.remaining().as_secs() == 0 {
                        // Ghost RESET when nothing to clear; drop the tap.
                        Action::None
                    } else {
                        let was_running = data.timer.is_running();
                        data.timer = TimerState::Idle {
                            duration: Duration::from_ticks(0),
                        };
                        self.last_rendered_sec = 0;
                        if was_running {
                            Action::CancelTimer
                        } else {
                            Action::Redraw
                        }
                    }
                } else {
                    Action::None
                }
            }

            _ => Action::None,
        }
    }
}

// -- Numpad view -------------------------------------------------------------

impl TimerScreen {
    fn render_numpad<D: DrawTarget<Color = Rgb565>>(&self, display: &mut D, data: &SystemData) {
        draw_app_chrome(display, data, "SET TIMER", TELEMETRY, ACCENT);

        // Time label - shows the digit-buffer's HH:MM:SS during entry,
        // or the post-clamp capped duration during the flash.
        let mut buf: heapless::String<12> = heapless::String::new();
        if self.flash_ticks > 0 {
            format_duration(data.timer.remaining(), &mut buf);
        } else {
            format_duration(digits_to_duration(&self.numpad.digits), &mut buf);
        }
        fonts::draw_centered(
            display, &fonts::value(),
            buf.as_str(),
            theme::SCREEN_W as i32 / 2, NUMPAD_TIME_Y,
            self.time_label_color(),
        );

        // Numpad button grid (still red-tinted - independent of accent).
        self.numpad.render(display);
    }

    fn numpad_event(&mut self, event: &SystemEvent, data: &mut SystemData) -> Action {
        match event {
            // Flash animation: count ticks, redraw on phase change.
            SystemEvent::MotionUpdated { .. } if self.flash_ticks > 0 => {
                let old_phase = self.flash_ticks / FLASH_PHASE_TICKS;
                self.flash_ticks -= 1;
                let new_phase = self.flash_ticks / FLASH_PHASE_TICKS;
                if new_phase != old_phase { Action::Redraw } else { Action::None }
            }

            // Header chevron: discard and return to Main.
            SystemEvent::Tap { x, y } if app_chrome_back_hit(*x, *y) => {
                self.flash_ticks = 0;
                self.view = TimerView::Main;
                Action::Redraw
            }

            SystemEvent::Tap { x, y } => {
                if let Some(action) = self.numpad.hit_test(*x, *y) {
                    match action {
                        NumpadAction::Confirm => {
                            let dur = digits_to_duration(&self.numpad.digits);
                            if dur.as_secs() > MAX_TIMER_SECS {
                                let capped = Duration::from_secs(MAX_TIMER_SECS);
                                data.timer = TimerState::Idle { duration: capped };
                                self.numpad.prefill(&duration_to_raw(capped));
                                self.flash_ticks = FLASH_TOTAL_TICKS;
                                return Action::Redraw;
                            }
                            data.timer = TimerState::Idle { duration: dur };
                            self.view = TimerView::Main;
                            Action::Redraw
                        }
                        other => {
                            if self.numpad.apply(other) {
                                Action::Redraw
                            } else {
                                Action::None
                            }
                        }
                    }
                } else {
                    Action::None
                }
            }

            _ => Action::None,
        }
    }
}

// -- Helpers -----------------------------------------------------------------

fn readout_rect() -> Rectangle {
    Rectangle::new(
        Point::new(SIDE_MARGIN, READOUT_TOP),
        Size::new(
            (theme::SCREEN_W as i32 - SIDE_MARGIN * 2) as u32,
            READOUT_H as u32,
        ),
    )
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

/// Format a duration as `HH:MM:SS` into the provided buffer.
fn format_duration(d: Duration, buf: &mut heapless::String<12>) {
    let total_secs = d.as_secs();
    let h = (total_secs / 3600).min(99);
    let m = (total_secs / 60) % 60;
    let s = total_secs % 60;
    let _ = write!(buf, "{:02}:{:02}:{:02}", h, m, s);
}

/// Convert a digit buffer (entered left-to-right) into a Duration.
/// Digits fill right-to-left: `[1, 3, 0]` → `00:01:30` = 90 seconds.
fn digits_to_duration(digits: &[u8]) -> Duration {
    let mut padded = [0u8; 6];
    let offset = 6 - digits.len();
    for (i, &d) in digits.iter().enumerate() {
        padded[offset + i] = d;
    }
    let h = padded[0] as u64 * 10 + padded[1] as u64;
    let m = padded[2] as u64 * 10 + padded[3] as u64;
    let s = padded[4] as u64 * 10 + padded[5] as u64;
    Duration::from_secs(h * 3600 + m * 60 + s)
}

/// Convert a Duration into a raw 6-digit array `[H, H, M, M, S, S]`.
fn duration_to_raw(d: Duration) -> [u8; 6] {
    let total_secs = d.as_secs();
    let h = (total_secs / 3600).min(99);
    let m = (total_secs / 60) % 60;
    let s = total_secs % 60;
    [
        (h / 10) as u8, (h % 10) as u8,
        (m / 10) as u8, (m % 10) as u8,
        (s / 10) as u8, (s % 10) as u8,
    ]
}
