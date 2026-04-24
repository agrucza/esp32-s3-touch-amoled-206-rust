//! Core UI types - Screen trait, actions, and shared data.

use embedded_graphics::{draw_target::DrawTarget, pixelcolor::Rgb565};

// Re-export self-test types so screens can pull them from a single
// place alongside the other UI data structs below. `pub use` also
// brings them into local scope for `SystemData`'s field types, so
// no separate `use` is needed.
pub use crate::events::{
    NUM_SELF_TESTS, SelfTestId, SelfTestResult, SystemEvent,
};

// -- Display power-management state ------------------------------------------

/// Display power-management state. Transitions are driven by idle
/// time since the last user-input event (touch / swipe / button).
///
/// * `Active`: normal running state at full brightness.
/// * `Dim`: brightness register dropped, rendering continues normally.
///   This is the first-stage power save and is the cheapest to enter
///   and leave (single DCS command over SPI).
/// * `Off`: `DISPOFF` issued, the entire render path is skipped until
///   a user event wakes the display again. Deepest power save short
///   of a full light-sleep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayState {
    Active,
    Dim,
    Off,
}

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
    /// Quick Access pull-down overlay (brightness + toggle tiles).
    /// Reached via swipe-down-from-top. Not part of the home-row
    /// rotation. Constructed via `ActiveScreen::new_quick_access(previous)`
    /// because it needs context that plain `new(id)` doesn't provide.
    QuickAccess,
    /// Pull-up app drawer (3x3 tile launcher). Reached via
    /// swipe-up-from-bottom and by tapping the watch face. Also
    /// overlay-like: constructed via `ActiveScreen::new_app_drawer(previous)`.
    AppDrawer,
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
    /// [`ScreenId::QuickAccess`] or [`ScreenId::AppDrawer`] (modal
    /// replace-top semantics for both overlays).
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
    /// Wipe user-visible persistent data (config, alarms, logs,
    /// uploaded sounds) back to defaults. The manager calls
    /// `FlashFs::reset_user_data`, re-summarises usage, and emits
    /// a fresh `SystemEvent::StorageUsageUpdated`. Irrecoverable -
    /// wrap in a confirmation dialog on the UI side.
    FactoryReset,
    /// (Re-)probe the SD card. Emitted by the Storage settings
    /// screen when the user taps the "SD CARD" row. The manager
    /// probes the card, flips the mirror online flag, runs back-fill
    /// if newly online, and emits a fresh
    /// `SystemEvent::StorageUsageUpdated` so the screen reflects
    /// the new status.
    InitSd,
    /// Restore the in-flash config blobs from the SD mirror, then
    /// software-reset. Destructive; the UI wraps the trigger in a
    /// confirm tap. Requires SD to be online - the Settings row is
    /// disabled otherwise.
    RestoreFromSd,
    /// Persist the current `AlarmState` to flash. Emitted by
    /// screens after they mutate `data.alarms`. Subsumes Redraw -
    /// returning this also triggers a redraw, so screens don't
    /// need to emit both.
    PersistAlarms,
    /// Persist the current `Config` to flash. Same subsumes-Redraw
    /// semantics as `PersistAlarms`. Emitted internally by the
    /// Model after a `SetBrightness`-style mutation; screens can
    /// also return this directly after editing other Config fields.
    PersistConfig,

    /// Set the display brightness to `percent` (5..=100 slider range).
    /// Applies live to the panel and updates the in-memory `Config`
    /// + `SystemData` snapshot. Model also marks config dirty; the
    /// eventual save happens on the next `TouchReleased`, so finger
    /// scrubbing doesn't hammer flash.
    SetBrightness { percent: u8 },

    /// Flip `config.display.night_mode`. Model applies the new
    /// effective brightness to the panel immediately (night mode
    /// caps at `DisplayConfig::NIGHT_MODE_MAX_HW`) and marks config
    /// dirty so the change persists on the next `TouchReleased`.
    ToggleNightMode,
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
///
/// When serialised (via the `serde` feature) only `entries` is
/// written - `active_hw` / `alerting` / `snoozed` are transient
/// runtime flags that must NOT persist across a reboot, so they
/// are `#[serde(skip)]` and reset to `Default` on load.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AlarmState {
    pub entries: [AlarmEntry; MAX_ALARMS],
    /// Index of the alarm currently programmed into the RTC
    /// hardware, or None if no alarm is active. Recomputed on
    /// boot by `plan_reprogram` from current time + entries.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub active_hw: Option<usize>,
    /// True when an alarm has fired and the user hasn't dismissed
    /// it. Not persisted: a mid-alarm reboot should not resume
    /// ringing.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub alerting: bool,
    /// True when a snooze is active. The manager skips regular
    /// reprogramming while this is set. Cleared when the snooze
    /// alarm fires. Not persisted: mid-snooze reboot cancels it.
    #[cfg_attr(feature = "serde", serde(skip))]
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

    /// Compute the clock-time (hour, minute) for a snooze alarm
    /// firing `minutes_from_now` minutes after the given
    /// `hour`:`minute`. Handles hour and midnight wraparound.
    ///
    /// Associated function (no `self`) so the snooze duration is
    /// decided by the caller and the calculation is easy to host-test.
    pub fn compute_snooze(hour: u8, minute: u8, minutes_from_now: u8) -> (u8, u8) {
        let total = minute as u16 + minutes_from_now as u16;
        let snooze_minute = (total % 60) as u8;
        let hours_added = (total / 60) as u8;
        let snooze_hour = (hour + hours_added) % 24;
        (snooze_hour, snooze_minute)
    }

    /// Decide what the RTC should be programmed to based on the
    /// current time and the list of enabled alarms. Mutates
    /// `active_hw` to the new target (so subsequent ticks with
    /// the same inputs return `None`) and returns the command
    /// the caller should forward to the RTC driver.
    ///
    /// Returns `None` when snoozed (the snooze alarm is already
    /// in the RTC and must not be overwritten) or when nothing
    /// changed since the last call.
    pub fn plan_reprogram(
        &mut self,
        hour: u8,
        minute: u8,
        weekday: u8,
    ) -> Option<AlarmReprogram> {
        if self.snoozed {
            return None;
        }
        let next = self.next_alarm(hour, minute, weekday);
        if next == self.active_hw {
            return None;
        }
        self.active_hw = next;
        Some(match next {
            Some(idx) => {
                let e = &self.entries[idx];
                AlarmReprogram::SetAlarm { hour: e.hour, minute: e.minute }
            }
            None => AlarmReprogram::CancelAlarm,
        })
    }
}

/// Result of [`AlarmState::plan_reprogram`]: what the caller
/// should command the RTC driver to do now. Opaque to `app-core`
/// beyond being the enum; `firmware` translates this into the
/// concrete `RtcCommand` channel messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlarmReprogram {
    /// Program the RTC to fire at the given hour:minute.
    SetAlarm { hour: u8, minute: u8 },
    /// Clear the RTC alarm (no enabled alarms).
    CancelAlarm,
}

#[cfg(test)]
mod alarm_tests {
    use super::*;

    fn state_with(entries: &[(u8, u8, u8 /* days mask */, bool /* enabled */)]) -> AlarmState {
        let mut s = AlarmState::default();
        for (i, &(h, m, days, enabled)) in entries.iter().enumerate() {
            s.entries[i] = AlarmEntry { hour: h, minute: m, days, enabled };
        }
        s
    }

    #[test]
    fn snoozed_blocks_reprogram() {
        let mut s = state_with(&[(7, 0, 0b0111_1111, true)]);
        s.snoozed = true;
        assert_eq!(s.plan_reprogram(6, 0, 0), None);
    }

    #[test]
    fn fresh_alarm_produces_set_command() {
        let mut s = state_with(&[(7, 30, 0b0111_1111, true)]);
        assert_eq!(
            s.plan_reprogram(6, 0, 0),
            Some(AlarmReprogram::SetAlarm { hour: 7, minute: 30 }),
        );
        assert_eq!(s.active_hw, Some(0));
    }

    #[test]
    fn idempotent_when_nothing_changed() {
        let mut s = state_with(&[(7, 30, 0b0111_1111, true)]);
        assert!(s.plan_reprogram(6, 0, 0).is_some());
        // Subsequent call with no change returns None.
        assert_eq!(s.plan_reprogram(6, 0, 0), None);
    }

    #[test]
    fn snooze_adds_minutes_no_wrap() {
        assert_eq!(AlarmState::compute_snooze(7, 30, 10), (7, 40));
    }

    #[test]
    fn snooze_wraps_minutes_into_next_hour() {
        assert_eq!(AlarmState::compute_snooze(7, 55, 10), (8, 5));
    }

    #[test]
    fn snooze_wraps_past_midnight() {
        // 23:55 + 10 min = 00:05
        assert_eq!(AlarmState::compute_snooze(23, 55, 10), (0, 5));
    }

    #[test]
    fn snooze_with_larger_delay_spans_multiple_hours() {
        // 22:00 + 180 min = 01:00 next day
        assert_eq!(AlarmState::compute_snooze(22, 0, 180), (1, 0));
    }

    #[test]
    fn disabling_all_alarms_produces_cancel() {
        let mut s = state_with(&[(7, 30, 0b0111_1111, true)]);
        assert!(s.plan_reprogram(6, 0, 0).is_some());
        // Disable the alarm.
        s.entries[0].enabled = false;
        assert_eq!(s.plan_reprogram(6, 0, 0), Some(AlarmReprogram::CancelAlarm));
        assert_eq!(s.active_hw, None);
    }
}

// -- System data snapshot ----------------------------------------------------

// Per-peripheral snapshot data structs live in `app-core::data`.
// Re-exported here so screens can `use crate::ui::types::{TimeData,
// PowerData, MotionData, TouchData, StorageUsage, SystemData}` from
// one place.
pub use crate::data::{MotionData, PowerData, StorageUsage, TimeData, TouchData};

/// System state, passed to screens on render and events.
///
/// Peripheral snapshots are updated by the manager's event handler.
/// App state (stopwatch, timer) is mutated directly by screens
/// via `&mut SystemData` in `on_event`.
///
/// Intentionally not `Copy`: [`crate::data::BatteryHistory`] owns a
/// ring buffer that can't live on the stack silently on every pass.
/// The struct is always accessed through `&SystemData` / `&mut
/// SystemData` references, so the missing `Copy` costs nothing at
/// call sites - see `Model::new` (only by-value use).
#[derive(Debug, Clone, Default)]
pub struct SystemData {
    pub time: TimeData,
    pub power: PowerData,
    pub motion: MotionData,
    pub touch: TouchData,
    /// Flash-filesystem occupancy. Updated at boot and after
    /// every save / reset via `SystemEvent::StorageUsageUpdated`.
    pub storage: StorageUsage,
    /// Recent battery-percent samples for the settings battery
    /// graph. Seeded at boot from the flash event log, appended
    /// on every `SystemEvent::BatteryChanged`.
    pub battery_history: crate::data::BatteryHistory,
    pub tick_count: u32,
    pub stopwatch: StopwatchState,
    pub timer: TimerState,
    pub alarms: AlarmState,
    /// Per-test latest result, indexed by [`SelfTestId`] cast to
    /// `usize`. Updated by the main loop whenever a
    /// [`SystemEvent::SelfTestUpdated`] arrives.
    pub self_tests: [SelfTestResult; NUM_SELF_TESTS],
    /// Read-only snapshot of the runtime `Config`. Kept in sync by
    /// the Model so any screen can render `data.config.*` without
    /// its own backing store or constructor parameter. Mutation
    /// goes through `Action::PersistConfig` / `Action::SetBrightness`
    /// etc., never by a screen editing this field.
    pub config: crate::config::Config,
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
