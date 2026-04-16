//! Stopwatch screen - count-up timer as a panel app.
//!
//! Layout: standard header bar (X close on the left, "STOPWATCH"
//! title on the right), an amber hero pill containing HH:MM:SS
//! elapsed time rendered in the hero font (matching the clock
//! face), and two large dark circles below (Start/Pause on the
//! left, Reset on the right). The left circle's glyph + caption
//! switches based on run state (play glyph + "START" when
//! idle/paused, pause glyph + "PAUSE" while running).
//!
//! All positioning uses the shared `layout` constants (hero pill,
//! circle pair, glyph radius) so the visual rhythm matches the
//! clock home face exactly.
//!
//! Navigation: tapping the X icon in the header closes the screen
//! and returns via `Action::Back` (nav stack pop).
//!
//! The display ticks at ~20 Hz while running by returning
//! `Action::Redraw` from every [`SystemEvent::MotionUpdated`] event
//! (the IMU task's periodic cadence). That gives 50 ms display
//! resolution, which is plenty for a second-resolution stopwatch.
//!
//! Known limitation: state is owned by [`StopwatchScreen`], which
//! means leaving the screen (via the panel pull-down to another
//! app) drops the StopwatchScreen and next time you return it
//! starts fresh at 00:00:00. A durable stopwatch that keeps running
//! while you use other apps would need state hoisted out into a
//! task-owned struct. We'll do that when we need it.

use core::fmt::Write;

use embassy_time::{Duration, Instant};
use embedded_graphics::{
    draw_target::DrawTarget,
    pixelcolor::Rgb565,
};

use crate::events::SystemEvent;
use crate::ui::{fonts, glyphs, layout, primitives, theme};
use crate::ui::types::{Action, Screen, SystemData};
use crate::ui::widgets::{header_bar, icon_button, HeaderIcon};

// -- State machine ----------------------------------------------------------

/// Stopwatch run state. `Idle` shows 00:00:00 frozen. `Running`
/// holds the instant the current run segment started plus any
/// `accumulated` duration from prior runs we've paused and resumed
/// from. `Paused` freezes a total elapsed duration.
#[derive(Debug, Clone, Copy)]
enum RunState {
    Idle,
    Running { start: Instant, accumulated: Duration },
    Paused  { accumulated: Duration },
}

impl RunState {
    /// Total elapsed duration regardless of current state.
    fn elapsed(&self) -> Duration {
        match self {
            Self::Idle => Duration::from_ticks(0),
            Self::Running { start, accumulated } => {
                *accumulated + Instant::now().duration_since(*start)
            }
            Self::Paused { accumulated } => *accumulated,
        }
    }
}

// -- Screen -----------------------------------------------------------------

pub struct StopwatchScreen {
    state: RunState,
    /// Last displayed elapsed second, to avoid redundant redraws.
    last_rendered_sec: u64,
}

impl StopwatchScreen {
    pub fn new() -> Self {
        Self { state: RunState::Idle, last_rendered_sec: 0 }
    }
}

impl Screen for StopwatchScreen {
    fn render<D: DrawTarget<Color = Rgb565>>(&self, display: &mut D, _data: &SystemData) {
        // -- Header bar (X close left, title right) -----------------------
        header_bar(
            display,
            layout::header_rect(),
            HeaderIcon::Close,
            "STOPWATCH",
            theme::AMBER,
        );

        // -- Amber hero pill with HH:MM:SS --------------------------------
        primitives::pill_solid(
            display,
            layout::HERO_PILL_X, layout::HERO_PILL_Y,
            layout::HERO_PILL_W, layout::HERO_PILL_H,
            theme::AMBER,
        );
        let elapsed = self.state.elapsed();
        let total_secs = elapsed.as_secs();
        let hours   = (total_secs / 3600).min(99);
        let minutes = (total_secs / 60) % 60;
        let seconds = total_secs % 60;
        let mut time_buf: heapless::String<12> = heapless::String::new();
        let _ = write!(time_buf, "{:02}:{:02}:{:02}", hours, minutes, seconds);
        fonts::draw_centered_in_rect(
            display, &fonts::hero(),
            &time_buf, layout::HERO_RECT,
            theme::BG,
        );

        // -- Left circle: Start/Pause toggle ------------------------------
        match self.state {
            RunState::Running { .. } => icon_button(
                display,
                layout::LEFT_CIRCLE_CX, layout::CIRCLE_CY,
                theme::PANEL_BG,
                glyphs::pause, theme::TEXT_WHITE,
                "PAUSE", theme::TEXT_DIM,
            ),
            RunState::Idle | RunState::Paused { .. } => icon_button(
                display,
                layout::LEFT_CIRCLE_CX, layout::CIRCLE_CY,
                theme::PANEL_BG,
                glyphs::play, theme::TEXT_WHITE,
                "START", theme::TEXT_DIM,
            ),
        };

        // -- Right circle: Reset ------------------------------------------
        icon_button(
            display,
            layout::RIGHT_CIRCLE_CX, layout::CIRCLE_CY,
            theme::PANEL_BG,
            glyphs::stop, theme::TEXT_WHITE,
            "RESET", theme::TEXT_DIM,
        );
    }

    fn on_event(&mut self, event: &SystemEvent, _data: &SystemData) -> Action {
        match event {
            SystemEvent::PowerButtonLong => Action::Shutdown,

            // 20 Hz tick: redraw only when the displayed second changes.
            SystemEvent::MotionUpdated { .. }
                if matches!(self.state, RunState::Running { .. }) =>
            {
                let sec = self.state.elapsed().as_secs();
                if sec != self.last_rendered_sec {
                    self.last_rendered_sec = sec;
                    Action::Redraw
                } else {
                    Action::None
                }
            }

            // Header icon (X): pop the nav stack.
            SystemEvent::Tap { x, y } if layout::header_icon_hit(*x, *y) => {
                Action::Back
            }

            SystemEvent::Tap { x, y } => {
                if layout::left_circle_hit(*x, *y) {
                    self.state = match self.state {
                        RunState::Idle => RunState::Running {
                            start: Instant::now(),
                            accumulated: Duration::from_ticks(0),
                        },
                        RunState::Running { start, accumulated } => RunState::Paused {
                            accumulated: accumulated + Instant::now().duration_since(start),
                        },
                        RunState::Paused { accumulated } => RunState::Running {
                            start: Instant::now(),
                            accumulated,
                        },
                    };
                    Action::Redraw
                } else if layout::right_circle_hit(*x, *y) {
                    self.state = RunState::Idle;
                    Action::Redraw
                } else {
                    Action::None
                }
            }

            _ => Action::None,
        }
    }
}
