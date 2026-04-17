//! Timer screen - count-down timer with numpad duration entry.
//!
//! Two internal views (same pattern as Settings):
//!
//! **Main view** - standard hero pill + circle pair layout:
//! - Header bar: Close (X) + "TIMER"
//! - Hero pill: HH:MM:SS countdown in hero font. Tappable when
//!   idle or paused to open the numpad.
//! - Left circle: play/pause toggle (START/PAUSE/RESUME)
//! - Right circle: stop/reset
//!
//! **Numpad view** - digit entry for setting the duration:
//! - Header bar: Back chevron + "TIMER"
//! - Amber time label: HH:MM:SS showing entered digits
//!   (right-to-left fill like a calculator)
//! - 3x4 grid of rounded-rect buttons via the Numpad widget
//!
//! Tapping the hero pill in idle/paused opens the numpad. Confirming
//! on the numpad sets the duration and returns to the main view.
//! If the entered duration exceeds the hardware maximum (4h15m),
//! the value is clamped and the time label flashes red twice. The
//! user must confirm again with the capped value.

use core::fmt::Write;

use embassy_time::{Duration, Instant};
use embedded_graphics::{
    draw_target::DrawTarget,
    pixelcolor::Rgb565,
};

use crate::events::SystemEvent;
use crate::ui::{fonts, glyphs, layout, primitives, theme};
use crate::ui::types::{Action, Screen, SystemData, TimerState};
use crate::system::tasks::rtc::TimeData;
use crate::ui::widgets::{header_bar, icon_button, HeaderIcon, Numpad, NumpadAction};

// -- Constants ---------------------------------------------------------------

/// Y of the time label (top of glyphs) in the numpad view.
const NUMPAD_TIME_Y: i32 = 90;

/// Maximum timer duration in seconds (255 * 60s = 4h15m).
const MAX_TIMER_SECS: u64 = 15300;

/// Ticks per flash phase (250ms at 20 Hz = 5 ticks).
const FLASH_PHASE_TICKS: u8 = 5;

/// Total flash ticks (4 phases = 1 second).
const FLASH_TOTAL_TICKS: u8 = FLASH_PHASE_TICKS * 4;

// -- Internal types ----------------------------------------------------------

/// Which view the timer screen is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimerView {
    Main,
    Numpad,
}

// -- Screen ------------------------------------------------------------------

pub struct TimerScreen {
    view: TimerView,
    /// Last displayed remaining second, to avoid redundant redraws.
    last_rendered_sec: u64,
    /// Numpad widget for duration entry.
    numpad: Numpad,
    /// Remaining flash ticks. When > 0, the numpad time label
    /// alternates between amber and red to indicate the duration
    /// was clamped to the hardware maximum.
    flash_ticks: u8,
    /// True when the timer has expired and we're alerting the user.
    /// Any tap dismisses the alert and stops the buzz.
    alerting: bool,
    /// Tick counter for the alert flash animation. Incremented on
    /// each MotionUpdated while alerting.
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

    /// Compute remaining seconds from the current RTC time and the
    /// target. Handles midnight wrap.
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
    /// RTC time and set the embassy deadline.
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

    /// Returns the hero pill color while alerting: alternates
    /// amber/red at 250ms per phase (same rate as numpad flash).
    fn alert_pill_color(&self) -> Rgb565 {
        let phase = self.alert_ticks / FLASH_PHASE_TICKS;
        if phase % 2 == 0 {
            theme::AMBER
        } else {
            theme::RED
        }
    }

    /// Returns the numpad time label color, alternating amber/red
    /// during the clamp flash animation.
    fn time_label_color(&self) -> Rgb565 {
        if self.flash_ticks == 0 {
            return theme::AMBER;
        }
        let phase = (FLASH_TOTAL_TICKS - self.flash_ticks) / FLASH_PHASE_TICKS;
        if phase % 2 == 0 {
            theme::RED
        } else {
            theme::AMBER
        }
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
            return Action::BuzzStart { on_ms: 200, off_ms: 100 };
        }

        // While alerting: tick the flash, dismiss on tap.
        if self.alerting {
            if matches!(event, SystemEvent::Tap { .. }) {
                self.alerting = false;
                self.alert_ticks = 0;
                return Action::BuzzStop;
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

        // Resync embassy deadline from RTC time.
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
        header_bar(
            display,
            layout::header_rect(),
            HeaderIcon::Close,
            "TIMER",
            theme::AMBER,
        );

        // Hero pill with remaining time. Flashes red while alerting.
        let pill_color = if self.alerting {
            self.alert_pill_color()
        } else {
            theme::AMBER
        };
        primitives::pill_solid(
            display,
            layout::HERO_PILL_X, layout::HERO_PILL_Y,
            layout::HERO_PILL_W, layout::HERO_PILL_H,
            pill_color,
        );
        let rem = data.timer.remaining();
        let mut buf: heapless::String<12> = heapless::String::new();
        format_duration(rem, &mut buf);
        fonts::draw_centered_in_rect(
            display, &fonts::hero(),
            &buf, layout::HERO_RECT,
            theme::BG,
        );

        // Left circle: Start/Pause toggle.
        match data.timer {
            TimerState::Running { .. } => icon_button(
                display,
                layout::LEFT_CIRCLE_CX, layout::CIRCLE_CY,
                theme::PANEL_BG,
                glyphs::pause, theme::TEXT_WHITE,
                "PAUSE", theme::TEXT_DIM,
            ),
            _ => icon_button(
                display,
                layout::LEFT_CIRCLE_CX, layout::CIRCLE_CY,
                theme::PANEL_BG,
                glyphs::play, theme::TEXT_WHITE,
                "START", theme::TEXT_DIM,
            ),
        };

        // Right circle: Reset.
        icon_button(
            display,
            layout::RIGHT_CIRCLE_CX, layout::CIRCLE_CY,
            theme::PANEL_BG,
            glyphs::stop, theme::TEXT_WHITE,
            "RESET", theme::TEXT_DIM,
        );
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

            // Header X: close.
            SystemEvent::Tap { x, y } if layout::header_icon_hit(*x, *y) => {
                Action::Back
            }

            // Tap hero pill: open numpad (only when not running).
            SystemEvent::Tap { x, y }
                if !data.timer.is_running()
                    && layout::hero_pill_hit(*x, *y) =>
            {
                self.numpad.clear();
                duration_to_digits(data.timer.remaining(), &mut self.numpad.digits);
                self.view = TimerView::Numpad;
                Action::Redraw
            }

            SystemEvent::Tap { x, y } => {
                if layout::left_circle_hit(*x, *y) {
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
                        _ => Action::None,
                    }
                } else if layout::right_circle_hit(*x, *y) {
                    let was_running = data.timer.is_running();
                    data.timer = TimerState::Idle {
                        duration: Duration::from_ticks(0),
                    };
                    if was_running {
                        Action::CancelTimer
                    } else {
                        Action::Redraw
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
        header_bar(
            display,
            layout::header_rect(),
            HeaderIcon::Back,
            "TIMER",
            theme::AMBER,
        );

        // Time label showing entered digits as HH:MM:SS.
        // During a clamp flash, show the capped duration from state.
        let mut buf: heapless::String<12> = heapless::String::new();
        if self.flash_ticks > 0 {
            format_duration(data.timer.remaining(), &mut buf);
        } else {
            format_duration(digits_to_duration(&self.numpad.digits), &mut buf);
        }
        fonts::draw_centered(
            display, &fonts::value(),
            &buf,
            theme::SCREEN_W as i32 / 2, NUMPAD_TIME_Y,
            self.time_label_color(),
        );

        // Button grid.
        self.numpad.render(display);
    }

    fn numpad_event(&mut self, event: &SystemEvent, data: &mut SystemData) -> Action {
        match event {
            // Flash animation: count ticks, redraw on phase change.
            SystemEvent::MotionUpdated { .. } if self.flash_ticks > 0 => {
                let old_phase = self.flash_ticks / FLASH_PHASE_TICKS;
                self.flash_ticks -= 1;
                let new_phase = self.flash_ticks / FLASH_PHASE_TICKS;
                if new_phase != old_phase {
                    Action::Redraw
                } else {
                    Action::None
                }
            }

            // Back chevron in header: discard and return to main.
            SystemEvent::Tap { x, y } if layout::header_icon_hit(*x, *y) => {
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
                                self.numpad.clear();
                                duration_to_digits(capped, &mut self.numpad.digits);
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

// -- Duration / digit helpers ------------------------------------------------

/// Format a duration as HH:MM:SS into the provided buffer.
fn format_duration(d: Duration, buf: &mut heapless::String<12>) {
    let total_secs = d.as_secs();
    let h = (total_secs / 3600).min(99);
    let m = (total_secs / 60) % 60;
    let s = total_secs % 60;
    let _ = write!(buf, "{:02}:{:02}:{:02}", h, m, s);
}

/// Convert a digit buffer (entered left-to-right) into a Duration.
/// Digits fill right-to-left: [1, 3, 0] -> 00:01:30 = 90 seconds.
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

/// Convert a Duration into digits for populating the numpad buffer.
/// Strips leading zeros.
fn duration_to_digits(d: Duration, digits: &mut heapless::Vec<u8, 6>) {
    digits.clear();
    let total_secs = d.as_secs();
    let h = (total_secs / 3600).min(99);
    let m = (total_secs / 60) % 60;
    let s = total_secs % 60;
    let raw = [
        (h / 10) as u8, (h % 10) as u8,
        (m / 10) as u8, (m % 10) as u8,
        (s / 10) as u8, (s % 10) as u8,
    ];
    let mut started = false;
    for &d in &raw {
        if d != 0 { started = true; }
        if started {
            let _ = digits.push(d);
        }
    }
}
