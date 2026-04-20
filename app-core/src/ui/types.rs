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
    /// Alarm manager. Reachable from the clock face's ALARM circle
    /// and the panel.
    Alarm,
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
    /// `on_ms` on, `off_ms` off, repeated until `StopBuzz` is sent.
    StartBuzz { on_ms: u16, off_ms: u16 },
    /// Stop an active buzz pattern.
    StopBuzz,
    /// Snooze the active alarm. The manager stops the buzz, sets
    /// the snoozed flag, and programs the RTC with now + 10 minutes.
    SnoozeAlarm,
    /// Dismiss the active alarm. The manager stops the buzz and
    /// navigates back to the previous screen.
    DismissAlarm,
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

// -- Alarm state -------------------------------------------------------------

/// Maximum number of user-configurable alarms.
pub const MAX_ALARMS: usize = 8;

/// One alarm entry.
#[derive(Debug, Clone, Copy)]
pub struct AlarmEntry {
    pub hour: u8,
    pub minute: u8,
    /// Bitmask of active days: bit 0 = Sunday, bit 1 = Monday, ...
    /// bit 6 = Saturday. 0x7F = every day, 0x3E = Mon-Fri,
    /// 0x41 = Sat+Sun, 0x00 = disabled.
    pub days: u8,
    pub enabled: bool,
}

impl Default for AlarmEntry {
    fn default() -> Self {
        Self { hour: 0, minute: 0, days: 0x7F, enabled: false }
    }
}

impl AlarmEntry {
    /// True if this alarm fires on the given weekday (0=Sunday..6=Saturday).
    pub fn fires_on(&self, weekday: u8) -> bool {
        self.enabled && (self.days & (1 << weekday)) != 0
    }
}

/// Persistent alarm list. Screens mutate this directly.
#[derive(Debug, Clone, Copy)]
pub struct AlarmState {
    pub entries: [AlarmEntry; MAX_ALARMS],
    /// Index of the alarm currently programmed into the RTC hardware,
    /// or None if no alarm is active.
    pub active_hw: Option<usize>,
    /// True when an alarm has fired and the user hasn't dismissed it.
    pub alerting: bool,
    /// True when a snooze is active. The manager skips regular
    /// reprogramming while this is set. Cleared when the snooze
    /// alarm fires.
    pub snoozed: bool,
}

impl Default for AlarmState {
    fn default() -> Self {
        Self {
            entries: [AlarmEntry::default(); MAX_ALARMS],
            active_hw: None,
            alerting: false,
            snoozed: false,
        }
    }
}

impl AlarmState {
    /// Count of enabled alarms.
    pub fn enabled_count(&self) -> usize {
        self.entries.iter().filter(|e| e.enabled).count()
    }

    /// Find the next alarm that should fire after the given time
    /// and weekday. Returns the index, or None if no alarms are enabled.
    pub fn next_alarm(&self, hour: u8, minute: u8, weekday: u8) -> Option<usize> {
        let now_mins = hour as u16 * 60 + minute as u16;
        let mut best: Option<(usize, u16)> = None; // (index, minutes_until)

        for (i, entry) in self.entries.iter().enumerate() {
            if !entry.enabled {
                continue;
            }
            let alarm_mins = entry.hour as u16 * 60 + entry.minute as u16;

            // Check each of the next 7 days.
            for day_offset in 0u8..7 {
                let check_day = (weekday + day_offset) % 7;
                if !entry.fires_on(check_day) {
                    continue;
                }
                let mut mins_until = day_offset as u16 * 24 * 60 + alarm_mins;
                if day_offset == 0 && alarm_mins <= now_mins {
                    // Already passed today, try next week.
                    continue;
                }
                if day_offset == 0 {
                    mins_until = alarm_mins - now_mins;
                }
                match best {
                    Some((_, best_mins)) if mins_until < best_mins => {
                        best = Some((i, mins_until));
                    }
                    None => {
                        best = Some((i, mins_until));
                    }
                    _ => {}
                }
                break; // found the earliest firing day for this entry
            }
        }
        best.map(|(i, _)| i)
    }
}

// -- System data snapshot ----------------------------------------------------

// Per-peripheral snapshot data structs live in `app-core::data`.
// Re-exported here so screens can `use crate::ui::types::{TimeData,
// PowerData, MotionData, TouchData, SystemData}` from one place.
pub use crate::data::{MotionData, PowerData, TimeData, TouchData};

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
    pub alarms: AlarmState,
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
