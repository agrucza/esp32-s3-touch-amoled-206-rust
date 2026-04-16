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
//! - 3x4 grid of rounded-rect buttons: digits 0-9, backspace,
//!   confirm (play icon)
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
    geometry::{Point, Size},
    pixelcolor::Rgb565,
    prelude::Primitive,
    primitives::{Line, PrimitiveStyle, Triangle},
    Drawable,
};

use crate::events::SystemEvent;
use crate::ui::{fonts, glyphs, layout, primitives, theme};
use crate::ui::types::{Action, Screen, SystemData};
use crate::system::tasks::rtc::TimeData;
use crate::ui::widgets::{header_bar, icon_button, HeaderIcon};

// -- Numpad layout constants -------------------------------------------------

/// Y of the time label (top of glyphs) in the numpad view.
const NUMPAD_TIME_Y: i32 = 90;
/// Top of the numpad button grid, below the time label.
const NUMPAD_TOP: i32 = 150;
/// Button width.
const BTN_W: i32 = 90;
/// Button height.
const BTN_H: i32 = 52;
/// Horizontal gap between buttons.
const BTN_GAP_X: i32 = 10;
/// Vertical gap between buttons.
const BTN_GAP_Y: i32 = 8;
/// Corner radius for numpad buttons.
const BTN_RADIUS: u32 = 12;
/// Total grid width: 3 buttons + 2 gaps.
const GRID_W: i32 = 3 * BTN_W + 2 * BTN_GAP_X;
/// Left edge of the grid (horizontally centered).
const GRID_X: i32 = (theme::SCREEN_W as i32 - GRID_W) / 2;

/// The 4x3 button labels. Row-major order.
/// Bottom row: backspace (empty string, drawn as glyph), 0, confirm (empty, drawn as glyph).
const BUTTON_LABELS: [[&str; 3]; 4] = [
    ["1", "2", "3"],
    ["4", "5", "6"],
    ["7", "8", "9"],
    ["",  "0", ""],
];

// -- Internal types ----------------------------------------------------------

/// Which view the timer screen is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimerView {
    Main,
    Numpad,
}

/// Timer run state.
#[derive(Debug, Clone, Copy)]
enum RunState {
    /// Idle with a set duration (may be zero).
    Idle { duration: Duration },
    /// Counting down. `deadline` is an embassy Instant used for
    /// smooth 20Hz display updates between RTC syncs. Every 30s
    /// the `TimeUpdated` event resyncs the deadline from the real
    /// RTC time so drift never accumulates.
    Running { deadline: Instant },
    /// Paused with time remaining.
    Paused { remaining: Duration },
}

impl RunState {
    /// Remaining time, clamped to zero.
    fn remaining(&self) -> Duration {
        match self {
            Self::Idle { duration } => *duration,
            Self::Running { deadline } => {
                let now = Instant::now();
                if now >= *deadline {
                    Duration::from_ticks(0)
                } else {
                    deadline.duration_since(now)
                }
            }
            Self::Paused { remaining } => *remaining,
        }
    }
}

/// What a numpad tap resolved to.
enum NumpadKey {
    Digit(u8),
    Backspace,
    Confirm,
}

/// Maximum timer duration in seconds (255 * 60s = 4h15m).
const MAX_TIMER_SECS: u64 = 15300;

/// Ticks per flash phase (250ms at 20 Hz = 5 ticks).
const FLASH_PHASE_TICKS: u8 = 5;

/// Total flash ticks (4 phases = 1 second).
const FLASH_TOTAL_TICKS: u8 = FLASH_PHASE_TICKS * 4;

// -- Screen ------------------------------------------------------------------

pub struct TimerScreen {
    state: RunState,
    view: TimerView,
    /// Last displayed remaining second, to avoid redundant redraws.
    last_rendered_sec: u64,
    /// Digit buffer for numpad entry. Digits are pushed in order of
    /// entry but interpreted right-to-left (calculator style):
    /// entering [1, 3, 0] displays as 00:01:30.
    digits: heapless::Vec<u8, 6>,
    /// Remaining flash ticks. When > 0, the numpad time label
    /// alternates between amber and red to indicate the duration
    /// was clamped to the hardware maximum.
    flash_ticks: u8,
    /// Target time in seconds-since-midnight, set when the timer
    /// starts. Used to resync the embassy deadline from RTC time
    /// on every `TimeUpdated` event.
    target_secs: u32,
}

impl TimerScreen {
    pub fn new() -> Self {
        Self {
            state: RunState::Idle { duration: Duration::from_ticks(0) },
            view: TimerView::Main,
            last_rendered_sec: 0,
            digits: heapless::Vec::new(),
            flash_ticks: 0,
            target_secs: 0,
        }
    }

    /// Compute remaining seconds from the current RTC time and the
    /// stored target. Handles midnight wrap (target < now means it
    /// wrapped past 00:00:00).
    fn remaining_from_rtc(&self, time: &TimeData) -> u32 {
        let now_secs = time.hour as u32 * 3600
            + time.minute as u32 * 60
            + time.second as u32;
        if self.target_secs >= now_secs {
            self.target_secs - now_secs
        } else {
            // Wrapped past midnight.
            (24 * 3600 - now_secs) + self.target_secs
        }
    }

    /// Set up the running state: compute target_secs from current
    /// RTC time and set the embassy deadline for smooth display.
    fn start_countdown(&mut self, secs: u32, data: &SystemData) {
        let now_secs = data.time.hour as u32 * 3600
            + data.time.minute as u32 * 60
            + data.time.second as u32;
        self.target_secs = (now_secs + secs) % (24 * 3600);
        self.state = RunState::Running {
            deadline: Instant::now() + Duration::from_secs(secs as u64),
        };
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
    fn render<D: DrawTarget<Color = Rgb565>>(&self, display: &mut D, _data: &SystemData) {
        match self.view {
            TimerView::Main => self.render_main(display),
            TimerView::Numpad => self.render_numpad(display),
        }
    }

    fn on_event(&mut self, event: &SystemEvent, data: &SystemData) -> Action {
        if matches!(event, SystemEvent::PowerButtonLong) {
            return Action::Shutdown;
        }

        // RTC hardware timer expired - reset to idle.
        if matches!(event, SystemEvent::TimerExpired) {
            self.state = RunState::Idle {
                duration: Duration::from_ticks(0),
            };
            return Action::Redraw;
        }

        // Resync embassy deadline from RTC time every 30s.
        if let SystemEvent::TimeUpdated { data } = event {
            if let RunState::Running { .. } = self.state {
                let remaining = self.remaining_from_rtc(data);
                if remaining == 0 {
                    self.state = RunState::Idle {
                        duration: Duration::from_ticks(0),
                    };
                } else {
                    self.state = RunState::Running {
                        deadline: Instant::now() + Duration::from_secs(remaining as u64),
                    };
                }
                return Action::Redraw;
            }
        }

        match self.view {
            TimerView::Main => self.main_event(event, data),
            TimerView::Numpad => self.numpad_event(event),
        }
    }
}

// -- Main view ---------------------------------------------------------------

impl TimerScreen {
    fn render_main<D: DrawTarget<Color = Rgb565>>(&self, display: &mut D) {
        header_bar(
            display,
            layout::header_rect(),
            HeaderIcon::Close,
            "TIMER",
            theme::AMBER,
        );

        // Hero pill with remaining time.
        primitives::pill_solid(
            display,
            layout::HERO_PILL_X, layout::HERO_PILL_Y,
            layout::HERO_PILL_W, layout::HERO_PILL_H,
            theme::AMBER,
        );
        let rem = self.state.remaining();
        let mut buf: heapless::String<12> = heapless::String::new();
        format_duration(rem, &mut buf);
        fonts::draw_centered_in_rect(
            display, &fonts::hero(),
            &buf, layout::HERO_RECT,
            theme::BG,
        );

        // Left circle: Start/Pause toggle.
        match self.state {
            RunState::Running { .. } => icon_button(
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

    fn main_event(&mut self, event: &SystemEvent, data: &SystemData) -> Action {
        match event {
            // 20 Hz tick: redraw only when the displayed second changes.
            SystemEvent::MotionUpdated { .. }
                if matches!(self.state, RunState::Running { .. }) =>
            {
                let sec = self.state.remaining().as_secs();
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
                if !matches!(self.state, RunState::Running { .. })
                    && layout::hero_pill_hit(*x, *y) =>
            {
                self.digits.clear();
                duration_to_digits(self.state.remaining(), &mut self.digits);
                self.view = TimerView::Numpad;
                Action::Redraw
            }

            SystemEvent::Tap { x, y } => {
                if layout::left_circle_hit(*x, *y) {
                    match self.state {
                        RunState::Idle { duration } if duration.as_ticks() > 0 => {
                            let secs = duration.as_secs() as u32;
                            self.start_countdown(secs, data);
                            Action::StartTimer { seconds: secs }
                        }
                        RunState::Running { deadline } => {
                            let now = Instant::now();
                            let remaining = if now >= deadline {
                                Duration::from_ticks(0)
                            } else {
                                deadline.duration_since(now)
                            };
                            self.state = RunState::Paused { remaining };
                            Action::CancelTimer
                        }
                        RunState::Paused { remaining } if remaining.as_ticks() > 0 => {
                            let secs = remaining.as_secs() as u32;
                            self.start_countdown(secs, data);
                            Action::StartTimer { seconds: secs }
                        }
                        _ => Action::None,
                    }
                } else if layout::right_circle_hit(*x, *y) {
                    let was_running = matches!(self.state, RunState::Running { .. });
                    self.state = RunState::Idle {
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
    fn render_numpad<D: DrawTarget<Color = Rgb565>>(&self, display: &mut D) {
        header_bar(
            display,
            layout::header_rect(),
            HeaderIcon::Back,
            "TIMER",
            theme::AMBER,
        );

        // Time label showing entered digits as HH:MM:SS.
        // During a clamp flash, show the capped duration from state
        // rather than the raw digit buffer.
        let mut buf: heapless::String<12> = heapless::String::new();
        if self.flash_ticks > 0 {
            format_duration(self.state.remaining(), &mut buf);
        } else {
            format_duration(digits_to_duration(&self.digits), &mut buf);
        }
        fonts::draw_centered(
            display, &fonts::value(),
            &buf,
            theme::SCREEN_W as i32 / 2, NUMPAD_TIME_Y,
            self.time_label_color(),
        );

        // 3x4 button grid.
        for row in 0..4 {
            for col in 0..3 {
                let (bx, by) = button_origin(row, col);
                let label = BUTTON_LABELS[row][col];

                primitives::rounded_panel(
                    display,
                    bx, by, BTN_W, BTN_H, BTN_RADIUS,
                    None, Some(theme::AMBER),
                );

                if !label.is_empty() {
                    // Digit button: draw the label centered.
                    let rect = embedded_graphics::primitives::Rectangle::new(
                        Point::new(bx, by),
                        Size::new(BTN_W as u32, BTN_H as u32),
                    );
                    fonts::draw_centered_in_rect(
                        display, &fonts::value(),
                        label, rect,
                        theme::AMBER,
                    );
                } else if row == 3 && col == 0 {
                    // Backspace: small back chevron.
                    draw_backspace_glyph(display, bx + BTN_W / 2, by + BTN_H / 2);
                } else if row == 3 && col == 2 {
                    // Confirm: small play triangle.
                    draw_confirm_glyph(display, bx + BTN_W / 2, by + BTN_H / 2);
                }
            }
        }
    }

    fn numpad_event(&mut self, event: &SystemEvent) -> Action {
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
                if let Some(key) = numpad_hit(*x, *y) {
                    match key {
                        NumpadKey::Digit(d) => {
                            if self.digits.len() < 6 {
                                let _ = self.digits.push(d);
                                return Action::Redraw;
                            }
                        }
                        NumpadKey::Backspace => {
                            self.digits.pop();
                            return Action::Redraw;
                        }
                        NumpadKey::Confirm => {
                            let dur = digits_to_duration(&self.digits);
                            if dur.as_secs() > MAX_TIMER_SECS {
                                let capped = Duration::from_secs(MAX_TIMER_SECS);
                                self.state = RunState::Idle { duration: capped };
                                self.digits.clear();
                                duration_to_digits(capped, &mut self.digits);
                                self.flash_ticks = FLASH_TOTAL_TICKS;
                                return Action::Redraw;
                            } else {
                                self.state = RunState::Idle { duration: dur };
                                self.view = TimerView::Main;
                                return Action::Redraw;
                            }
                        }
                    }
                }
                Action::None
            }

            _ => Action::None,
        }
    }
}

// -- Numpad helpers ----------------------------------------------------------

/// Top-left corner of the button at (row, col).
fn button_origin(row: usize, col: usize) -> (i32, i32) {
    let x = GRID_X + col as i32 * (BTN_W + BTN_GAP_X);
    let y = NUMPAD_TOP + row as i32 * (BTN_H + BTN_GAP_Y);
    (x, y)
}

/// Hit-test the numpad grid, returning which key was tapped.
fn numpad_hit(x: u16, y: u16) -> Option<NumpadKey> {
    let px = x as i32;
    let py = y as i32;

    for row in 0..4 {
        for col in 0..3 {
            let (bx, by) = button_origin(row, col);
            if px >= bx && px < bx + BTN_W && py >= by && py < by + BTN_H {
                return match BUTTON_LABELS[row][col] {
                    "" if row == 3 && col == 0 => Some(NumpadKey::Backspace),
                    "" if row == 3 && col == 2 => Some(NumpadKey::Confirm),
                    "" => None,
                    s => {
                        let d = s.as_bytes()[0] - b'0';
                        Some(NumpadKey::Digit(d))
                    }
                };
            }
        }
    }
    None
}

/// Small back chevron for the backspace button.
fn draw_backspace_glyph<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, cx: i32, cy: i32,
) {
    let half = 8;
    let style = PrimitiveStyle::with_stroke(theme::AMBER, 2);
    Line::new(
        Point::new(cx + half, cy - half),
        Point::new(cx - half, cy),
    ).into_styled(style).draw(display).ok();
    Line::new(
        Point::new(cx - half, cy),
        Point::new(cx + half, cy + half),
    ).into_styled(style).draw(display).ok();
}

/// Small play triangle for the confirm button.
fn draw_confirm_glyph<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, cx: i32, cy: i32,
) {
    let r = 8;
    Triangle::new(
        Point::new(cx - r / 2, cy - r),
        Point::new(cx - r / 2, cy + r),
        Point::new(cx + r, cy),
    )
    .into_styled(PrimitiveStyle::with_fill(theme::AMBER))
    .draw(display).ok();
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
    // Pad to 6 digits, right-aligned.
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
    // Strip leading zeros but keep at least the last digit.
    let mut started = false;
    for &d in &raw {
        if d != 0 { started = true; }
        if started {
            let _ = digits.push(d);
        }
    }
}
