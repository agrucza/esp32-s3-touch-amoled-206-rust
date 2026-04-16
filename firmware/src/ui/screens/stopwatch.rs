//! Stopwatch screen - count-up timer as a panel app.
//!
//! Layout: standard settings-style header bar (X close on the
//! left, "STOPWATCH" title on the right), an amber hero pill
//! containing MM:SS.hh elapsed time rendered in a monospace bold
//! font so digits don't shimmer as they change, and two large
//! dark circles below (Start/Pause on the left, Reset on the
//! right). The left circle's glyph + caption switches based on
//! run state (play glyph + "START" when idle/paused, pause glyph
//! + "PAUSE" while running).
//!
//! Navigation: tapping the X icon in the header or swiping down
//! from the content area closes the screen and returns to Clock,
//! matching the Settings screen pattern.
//!
//! The display ticks at ~20 Hz while running by returning
//! `Action::Redraw` from every [`SystemEvent::MotionUpdated`] event
//! (the IMU task's periodic cadence). That gives 50 ms display
//! resolution, which is plenty for a MM:SS.hh stopwatch - the
//! rightmost digit pair (centiseconds) updates twice per tick.
//!
//! Known limitation: state is owned by [`StopwatchScreen`], which
//! means leaving the screen (via the panel pull-down to another
//! app) drops the StopwatchScreen and next time you return it
//! starts fresh at 00:00.00. A durable stopwatch that keeps running
//! while you use other apps would need state hoisted out into a
//! task-owned struct. We'll do that when we need it.

use core::fmt::Write;

use embassy_time::{Duration, Instant};
use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{Point, Size},
    pixelcolor::Rgb565,
    prelude::Primitive,
    primitives::{PrimitiveStyle, Rectangle, Triangle},
    Drawable,
};

use crate::events::{SwipeDir, SwipeRegion, SystemEvent};
use crate::ui::{fonts, layout, primitives, theme};
use crate::ui::types::{Action, Screen, ScreenId, SystemData};
use crate::ui::widgets::{header_bar, HeaderIcon};

// -- Layout constants (match clock.rs proportions) --------------------------

/// Width of the amber hero pill.
const HERO_PILL_W: i32 = 320;
/// Height of the amber hero pill.
const HERO_PILL_H: i32 = 130;
/// Top of the hero pill.
const HERO_PILL_Y: i32 = 160;
/// Left edge of the hero pill (horizontally centered).
const HERO_PILL_X: i32 = (theme::SCREEN_W as i32 - HERO_PILL_W) / 2;
/// Rect used to center the elapsed-time text inside the pill.
const HERO_RECT: Rectangle = Rectangle::new(
    Point::new(HERO_PILL_X, HERO_PILL_Y),
    Size::new(HERO_PILL_W as u32, HERO_PILL_H as u32),
);

/// Radius of each bottom circle.
const CIRCLE_RADIUS: i32 = 70;
/// Edge-to-edge gap between the two circles.
const CIRCLE_GAP: i32 = 24;
/// X center of the left (Start/Pause) circle.
const LEFT_CIRCLE_CX: i32 =
    theme::SCREEN_W as i32 / 2 - CIRCLE_GAP / 2 - CIRCLE_RADIUS;
/// X center of the right (Reset) circle.
const RIGHT_CIRCLE_CX: i32 =
    theme::SCREEN_W as i32 / 2 + CIRCLE_GAP / 2 + CIRCLE_RADIUS;
/// Vertical center of both circles.
const CIRCLE_CY: i32 = 310 + CIRCLE_RADIUS;
/// Glyph drawing radius (matches clock.rs tuning).
const GLYPH_RADIUS: i32 = CIRCLE_RADIUS - 37;
/// Gap between the bottom of a circle and the top of its caption.
const CIRCLE_LABEL_GAP: i32 = 14;

// -- State machine ----------------------------------------------------------

/// Stopwatch run state. `Idle` shows 00:00.00 frozen. `Running`
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
}

impl StopwatchScreen {
    pub fn new() -> Self {
        Self { state: RunState::Idle }
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

        // -- Amber hero pill with MM:SS.hh --------------------------------
        //
        // The hero font (`fub49`) is digits-only (`_tn` charset) so
        // it can't render the colon or decimal point. Use the
        // monospace `mono` font (Courier Bold 24) so the digit
        // columns stay pinned as digits change width.
        primitives::pill_solid(
            display,
            HERO_PILL_X, HERO_PILL_Y, HERO_PILL_W, HERO_PILL_H,
            theme::AMBER,
        );
        let elapsed = self.state.elapsed();
        let total_ms = elapsed.as_millis();
        let minutes = (total_ms / 60_000).min(99) as u32;
        let seconds = ((total_ms /  1_000) % 60) as u32;
        let centis  = ((total_ms /     10) % 100) as u32;
        let mut time_buf: heapless::String<12> = heapless::String::new();
        let _ = write!(time_buf, "{:02}:{:02}.{:02}", minutes, seconds, centis);
        fonts::draw_centered_in_rect(
            display, &fonts::mono(),
            &time_buf, HERO_RECT,
            theme::BG,
        );

        // -- Left circle: Start/Pause toggle ------------------------------
        primitives::circle_button(
            display,
            LEFT_CIRCLE_CX, CIRCLE_CY, CIRCLE_RADIUS,
            theme::PANEL_BG,
            None,
        );
        let (glyph_fn, left_label): (fn(&mut D, i32, i32, i32, Rgb565), &str) =
            match self.state {
                RunState::Running { .. } => (draw_pause_glyph, "PAUSE"),
                RunState::Idle | RunState::Paused { .. } => (draw_play_glyph, "START"),
            };
        glyph_fn(display, LEFT_CIRCLE_CX, CIRCLE_CY, GLYPH_RADIUS, theme::TEXT_WHITE);
        fonts::draw_centered(
            display, &fonts::caption(),
            left_label,
            LEFT_CIRCLE_CX, CIRCLE_CY + CIRCLE_RADIUS + CIRCLE_LABEL_GAP,
            theme::TEXT_DIM,
        );

        // -- Right circle: Reset ------------------------------------------
        primitives::circle_button(
            display,
            RIGHT_CIRCLE_CX, CIRCLE_CY, CIRCLE_RADIUS,
            theme::PANEL_BG,
            None,
        );
        draw_stop_glyph(
            display,
            RIGHT_CIRCLE_CX, CIRCLE_CY, GLYPH_RADIUS,
            theme::TEXT_WHITE,
        );
        fonts::draw_centered(
            display, &fonts::caption(),
            "RESET",
            RIGHT_CIRCLE_CX, CIRCLE_CY + CIRCLE_RADIUS + CIRCLE_LABEL_GAP,
            theme::TEXT_DIM,
        );
    }

    fn on_event(&mut self, event: &SystemEvent, _data: &SystemData) -> Action {
        match event {
            SystemEvent::PowerButtonLong => Action::Shutdown,

            // Motion-updated cadence (20 Hz from IMU task) keeps the
            // running display fresh. We don't read the motion data -
            // we just use the event as a periodic tick signal, and
            // only when actually counting.
            SystemEvent::MotionUpdated { .. }
                if matches!(self.state, RunState::Running { .. }) =>
            {
                Action::Redraw
            }

            // Header icon (X): pop the nav stack and return to
            // whatever screen opened the panel we were launched from.
            SystemEvent::Tap { x, y } if layout::header_icon_hit(*x, *y) => {
                Action::Back
            }

            // Swipe down in the content area closes the screen too
            // (mirrors the settings-screen "pull to dismiss" gesture).
            SystemEvent::Swipe {
                dir: SwipeDir::Down,
                region: SwipeRegion::Content,
            } => Action::Back,

            SystemEvent::Tap { x, y } => {
                if hit_left_circle(*x, *y) {
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
                } else if hit_right_circle(*x, *y) {
                    // Reset is always available - including mid-run,
                    // which stops-and-clears in one tap.
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

// -- Hit tests ---------------------------------------------------------------

fn hit_left_circle(x: u16, y: u16) -> bool {
    let dx = x as i32 - LEFT_CIRCLE_CX;
    let dy = y as i32 - CIRCLE_CY;
    dx * dx + dy * dy <= CIRCLE_RADIUS * CIRCLE_RADIUS
}

fn hit_right_circle(x: u16, y: u16) -> bool {
    let dx = x as i32 - RIGHT_CIRCLE_CX;
    let dy = y as i32 - CIRCLE_CY;
    dx * dx + dy * dy <= CIRCLE_RADIUS * CIRCLE_RADIUS
}

// -- Glyphs ------------------------------------------------------------------

/// Filled right-pointing triangle (play icon).
fn draw_play_glyph<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, cx: i32, cy: i32, radius: i32, color: Rgb565,
) {
    Triangle::new(
        Point::new(cx - radius / 2, cy - radius),
        Point::new(cx - radius / 2, cy + radius),
        Point::new(cx + radius,     cy),
    )
    .into_styled(PrimitiveStyle::with_fill(color))
    .draw(display).ok();
}

/// Two filled vertical bars (pause icon).
fn draw_pause_glyph<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, cx: i32, cy: i32, radius: i32, color: Rgb565,
) {
    let bar_w = (radius * 2 / 5).max(3);
    let bar_h = radius * 2;
    let gap = radius / 3;
    let fill = PrimitiveStyle::with_fill(color);
    Rectangle::new(
        Point::new(cx - gap - bar_w, cy - bar_h / 2),
        Size::new(bar_w as u32, bar_h as u32),
    ).into_styled(fill).draw(display).ok();
    Rectangle::new(
        Point::new(cx + gap, cy - bar_h / 2),
        Size::new(bar_w as u32, bar_h as u32),
    ).into_styled(fill).draw(display).ok();
}

/// Filled square (stop / reset icon).
fn draw_stop_glyph<D: DrawTarget<Color = Rgb565>>(
    display: &mut D, cx: i32, cy: i32, radius: i32, color: Rgb565,
) {
    let side = radius * 7 / 4;
    Rectangle::new(
        Point::new(cx - side / 2, cy - side / 2),
        Size::new(side as u32, side as u32),
    )
    .into_styled(PrimitiveStyle::with_fill(color))
    .draw(display).ok();
}
