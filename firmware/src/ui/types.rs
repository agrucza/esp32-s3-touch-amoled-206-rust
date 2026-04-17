//! Core UI types - Screen trait, actions, and shared data.

use embedded_graphics::{draw_target::DrawTarget, pixelcolor::Rgb565};

// Re-export self-test types so screens can pull them from a single
// place alongside the other UI data structs below. `pub use` also
// brings them into local scope for `SystemData`'s field types, so
// no separate `use` is needed.
pub use crate::events::{
    NUM_SELF_TESTS, SelfTestId, SelfTestResult, SystemEvent,
};

// -- Screen IDs --------------------------------------------------------------

/// Identifies which screen to switch to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenId {
    Clock,
    Status,
    /// Count-up stopwatch (HH:MM:SS). Panel-only app.
    Stopwatch,
    /// Count-down timer with numpad duration entry. Panel-only app,
    /// also reachable by tapping the TIMER circle on the clock face.
    Timer,
    /// Device settings - internally state-machined into sub-views
    /// (IMU, RTC, Power, ...) via `SettingsScreen`'s own enum, so
    /// from the outside there is only one screen id.
    Settings,
    /// The pull-down app picker. Not part of the home-row rotation
    /// (it's reached only via swipe-down-from-header) and constructed
    /// via `ActiveScreen::new_panel(previous)` because it needs
    /// context that plain `new(id)` doesn't provide.
    Panel,
}

// -- Actions -----------------------------------------------------------------

/// What a screen wants the system to do after processing an event.
///
/// Screens return an `Action` from `on_event` to tell the outer
/// navigator what to do next. The navigator is the only thing
/// allowed to mutate global system state (switch screens, signal
/// tasks, shut down) - screens never touch those directly. This
/// keeps screens portable and the control flow easy to trace.
///
/// ## Forward vs. back navigation
///
/// Screens that want to *go somewhere specific* return
/// [`SwitchScreen`]. The navigator pushes the current screen onto
/// its nav stack (unless the current screen is the Panel modal,
/// which is replaced rather than pushed) and switches to the
/// requested target.
///
/// Screens that want to *close and return to whatever opened them*
/// return [`Back`]. The navigator pops the nav stack and switches
/// to the popped id. When the stack is empty it falls back to
/// [`ScreenId::Clock`]. This is the right return for close X
/// buttons and swipe-to-dismiss gestures - screens don't hard-code
/// a target, so "close Stopwatch" goes back to Settings if that's
/// where the panel was opened from, or Clock if the user was there
/// before.
///
/// [`SwitchScreen`]: Action::SwitchScreen
/// [`Back`]: Action::Back
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Nothing to do.
    None,
    /// Screen content changed, request a display refresh.
    Redraw,
    /// Switch to a specific target screen. Pushes the current
    /// screen onto the nav stack unless the current screen is
    /// [`ScreenId::Panel`] (modal replace-top semantics).
    SwitchScreen(ScreenId),
    /// Pop the nav stack and return to the previous screen. Falls
    /// back to [`ScreenId::Clock`] when the stack is empty.
    Back,
    /// Request system shutdown.
    Shutdown,
    /// Run a hardware self-test. The main loop routes the id to the
    /// task that owns the underlying hardware and fires that task's
    /// command signal; the screen doesn't know which task handles it.
    /// Progress and results come back asynchronously as
    /// [`SystemEvent::SelfTestUpdated`] events.
    RunSelfTest(SelfTestId),
    /// Start the RTC hardware countdown timer. Duration is capped
    /// at 15300 seconds (4h15m) by the UI. When the timer expires
    /// the RTC task emits `SystemEvent::TimerExpired`.
    StartTimer { seconds: u32 },
    /// Cancel a running RTC countdown timer.
    CancelTimer,
    /// Set an RTC alarm at the given time. Optionally restrict to
    /// a single weekday (0=Sunday..6=Saturday). Fires
    /// `SystemEvent::AlarmFired` when matched.
    #[allow(dead_code)]
    SetAlarm { hour: u8, minute: u8, weekday: Option<u8> },
    /// Cancel a set RTC alarm.
    #[allow(dead_code)]
    CancelAlarm,
    /// Set the RTC date and time.
    SetTime { year: u16, month: u8, day: u8, hour: u8, minute: u8, second: u8 },
    /// Start a repeating haptic buzz pattern. The manager buzzes
    /// `on_ms` on, `off_ms` off, repeated until `BuzzStop` is sent.
    BuzzStart { on_ms: u16, off_ms: u16 },
    /// Stop an active buzz pattern.
    BuzzStop,
}

// -- Persistent app state ----------------------------------------------------

use embassy_time::{Duration, Instant};

/// Stopwatch run state, persisted across screen switches.
#[derive(Debug, Clone, Copy)]
pub enum StopwatchState {
    Idle,
    Running { start: Instant, accumulated: Duration },
    Paused { accumulated: Duration },
}

impl StopwatchState {
    /// Total elapsed duration regardless of current state.
    pub fn elapsed(&self) -> Duration {
        match self {
            Self::Idle => Duration::from_ticks(0),
            Self::Running { start, accumulated } => {
                *accumulated + Instant::now().duration_since(*start)
            }
            Self::Paused { accumulated } => *accumulated,
        }
    }

    /// True if the stopwatch is actively counting.
    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running { .. })
    }
}

impl Default for StopwatchState {
    fn default() -> Self { Self::Idle }
}

/// Timer run state, persisted across screen switches.
#[derive(Debug, Clone, Copy)]
pub enum TimerState {
    /// Idle with a set duration (may be zero).
    Idle { duration: Duration },
    /// Counting down toward a deadline. The embassy Instant is
    /// resynced from RTC time on every TimeUpdated event.
    /// `target_secs` is the absolute target in seconds-since-midnight,
    /// used for the RTC resync calculation.
    Running { deadline: Instant, target_secs: u32 },
    /// Paused with time remaining.
    Paused { remaining: Duration },
}

impl TimerState {
    /// Remaining time, clamped to zero.
    pub fn remaining(&self) -> Duration {
        match self {
            Self::Idle { duration } => *duration,
            Self::Running { deadline, .. } => {
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

    /// True if the timer is actively counting down.
    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running { .. })
    }
}

impl Default for TimerState {
    fn default() -> Self {
        Self::Idle { duration: Duration::from_ticks(0) }
    }
}

// -- System data snapshot ----------------------------------------------------

// The per-peripheral data structs live alongside the task that
// produces them, so each task module is a self-contained unit
// (hardware state + emitted data type). We just re-export them
// here so screens can `use crate::ui::types::{TimeData, PowerData,
// MotionData, TouchData, SystemData}` from one place.
pub use crate::system::tasks::imu::MotionData;
pub use crate::system::tasks::power::PowerData;
pub use crate::system::tasks::rtc::TimeData;
pub use crate::system::tasks::touch::TouchData;

/// System state, passed to screens on render and events.
///
/// Peripheral snapshots are updated by the manager's event handler.
/// App state (stopwatch, timer) is mutated directly by screens
/// via `&mut SystemData` in `on_event`.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemData {
    pub time: TimeData,
    pub power: PowerData,
    pub motion: MotionData,
    pub touch: TouchData,
    pub tick_count: u32,
    pub stopwatch: StopwatchState,
    pub timer: TimerState,
    /// Per-test latest result, indexed by [`SelfTestId`] cast to
    /// `usize`. Updated by the main loop whenever a
    /// [`SystemEvent::SelfTestUpdated`] arrives.
    pub self_tests: [SelfTestResult; NUM_SELF_TESTS],
}

// -- Screen trait -------------------------------------------------------------

/// Trait that all UI screens implement.
///
/// Screens are stateful - they can track animations, scroll positions,
/// selection state, etc. The SystemManager doesn't know or care about
/// screen internals.
///
/// ## Lifecycle
///
/// - [`on_mount`] runs once right after the screen is switched to, with
///   the current [`SystemData`] available. Use it to read initial state
///   (e.g. a diagnostics screen that kicks off a self-test, a file
///   explorer that reads the current directory) before the first render.
/// - [`on_unmount`] runs once right before the screen is swapped out
///   or dropped. Use it to release resources or persist state.
/// - [`render`] is called every frame. Must be a pure function of the
///   screen's own state plus the provided [`SystemData`] snapshot.
/// - [`on_event`] is called for every [`SystemEvent`] the main loop
///   receives while this screen is active. Returns an [`Action`]
///   telling the outer navigator what to do next.
///
/// Default implementations are provided for the lifecycle hooks so
/// screens only override what they need - `render` and usually
/// `on_event` are the only methods most screens have to provide.
///
/// [`on_mount`]: Screen::on_mount
/// [`on_unmount`]: Screen::on_unmount
/// [`render`]: Screen::render
/// [`on_event`]: Screen::on_event
pub trait Screen {
    /// Called once when this screen becomes active, before the first
    /// render. Read anything that needs to be loaded on open here.
    fn on_mount(&mut self, _data: &SystemData) {}

    /// Called once when this screen is about to be swapped out or
    /// dropped. Release resources or persist state here.
    fn on_unmount(&mut self) {}

    /// Render the screen to the display. Called every frame.
    fn render<D: DrawTarget<Color = Rgb565>>(&self, display: &mut D, data: &SystemData);

    /// Handle a system event. Return an Action to tell the manager what to do.
    /// `data` is mutable so screens can update shared persistent state
    /// (stopwatch, timer) directly.
    fn on_event(&mut self, event: &SystemEvent, data: &mut SystemData) -> Action;
}
