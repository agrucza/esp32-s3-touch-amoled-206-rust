//! Application state machine - hardware-agnostic.
//!
//! `Model` owns everything the app logically *is*: cached sensor
//! snapshots, the current screen, nav stack, display mode, sleep
//! flag, buzz pattern, config. It exposes two main entry points:
//!
//!   * [`Model::handle_event`] - fold a [`SystemEvent`] into state
//!     and return a list of [`Effect`]s for the caller to enact.
//!   * [`Model::tick`] - advance time-driven state (buzz phase
//!     transitions, dim/sleep idle timers) and return effects.
//!
//! The manager on the firmware side executes the returned effects
//! by calling hardware (display transitions, RTC signal channels,
//! motor GPIO, shutdown, etc.). Nothing in this module touches
//! hardware directly, so the full dispatch loop can be unit-tested
//! on the host.

use embassy_time::{Duration, Instant};
use heapless::Vec;

use crate::buzz::{BuzzAction, BuzzPattern};
use crate::config::Config;
use crate::data::TouchData;
use crate::events::{
    self, SelfTestId, SwipeDir, SwipeRegion, SystemEvent, NUM_SELF_TESTS,
};
use crate::nav::NavStack;
use crate::ui::screens::{self, ActiveScreen};
use crate::ui::types::{
    Action, AlarmReprogram, AlarmState, DisplayState, ScreenId, SystemData, TimerState,
};

/// Upper bound on the number of [`Effect`]s produced by a single
/// event/tick. In practice even the heaviest handlers emit 2-3.
pub const MAX_EFFECTS_PER_CALL: usize = 8;

/// Fixed-size buffer of effects returned by `Model` methods.
pub type Effects = Vec<Effect, MAX_EFFECTS_PER_CALL>;

/// What the caller should do to hardware after a `Model` call.
///
/// Carries primitive payload fields rather than firmware-side enum
/// types so `app-core` doesn't need to know how the manager wires
/// each effect to its concrete channel / GPIO / driver call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Transition the display between two power states (async on
    /// the manager side - issues DCS commands over SPI).
    TransitionDisplay { from: DisplayState, to: DisplayState },

    /// Broadcast that the system has entered sleep. Subscribers
    /// (touch/IMU/power tasks) switch to their low-power modes.
    BroadcastSleeping,
    /// Broadcast that the system has exited sleep.
    BroadcastAwake,
    /// Halt the CPU in hardware light sleep. Manager returns from
    /// the `rtc.sleep()` call when a wake source fires.
    EnterLightSleep,

    /// Motor GPIO on / off (one-shot edge).
    MotorOn,
    MotorOff,
    /// Short pulse: motor on, blocking-delay `duration_ms`, motor off.
    /// Used for the BOOT-press "going to sleep" haptic.
    MotorPulse { duration_ms: u32 },

    /// Send `RtcCommand::SetAlarm { hour, minute, weekday }`.
    SetAlarm { hour: u8, minute: u8, weekday: Option<u8> },
    /// Send `RtcCommand::CancelAlarm`.
    CancelAlarm,
    /// Send `RtcCommand::StartTimer { seconds }`.
    StartTimer { seconds: u32 },
    /// Send `RtcCommand::CancelTimer`.
    CancelTimer,
    /// Send `RtcCommand::SetTime { ... }`.
    SetTime { year: u16, month: u8, day: u8, hour: u8, minute: u8, second: u8 },

    /// Route a self-test to its owning hardware task's command
    /// channel. Today every id is IMU-owned; SD/PMU/RTC tests will
    /// add more routes at the manager.
    RunSelfTest(SelfTestId),

    /// Immediate shutdown request (Action::Shutdown from a screen).
    Shutdown,
}

/// Application state machine.
///
/// Fields are `pub` for host tests to poke at; the firmware side
/// only reads via the explicit accessors and mutates via
/// [`Self::handle_event`] / [`Self::tick`] / [`Self::clear_redraw`].
pub struct Model {
    /// Cached system snapshot. Event handlers update individual
    /// fields as snapshot events arrive; the render path reads
    /// this cache.
    pub cached_data: SystemData,

    /// The active screen.
    pub screen: ActiveScreen,

    /// LIFO nav stack for `Action::Back`.
    pub nav_stack: NavStack,

    /// Current display state - tracked in step with the hardware
    /// by the manager's effect executor.
    pub display_state: DisplayState,

    /// Wall-clock timestamp of the last user-input event. Used by
    /// the display state machine to decide when to dim / blank
    /// the panel.
    pub last_activity: Instant,

    /// System-wide sleep flag. When `true`, the display is forced
    /// Off and the IMU is in WoM mode regardless of idle time.
    pub sleeping: bool,

    /// Flag set by event handlers that visually matter. Consumed
    /// by the render path and cleared via [`Self::clear_redraw`].
    pub needs_redraw: bool,

    /// Runtime configuration (display dim/off timeouts, etc).
    pub config: Config,

    /// Active buzz pattern state, if any.
    pub buzz: Option<BuzzPattern>,
}

impl Model {
    /// Build a fresh model with the supplied initial snapshot and
    /// config. The initial screen is Clock; its `on_mount` hook is
    /// fired so any state it seeds from `cached_data` is ready
    /// before the first render.
    pub fn new(cached_data: SystemData, config: Config, now: Instant) -> Self {
        let mut screen = ActiveScreen::new(ScreenId::Clock);
        screen.mount(&cached_data);
        Self {
            cached_data,
            screen,
            nav_stack: NavStack::new(),
            display_state: DisplayState::Active,
            last_activity: now,
            sleeping: false,
            needs_redraw: true, // first frame always draws
            config,
            buzz: None,
        }
    }

    /// Current render-needed flag.
    pub fn needs_redraw(&self) -> bool {
        self.needs_redraw
    }

    /// Reset the redraw flag. Called after a successful render.
    pub fn clear_redraw(&mut self) {
        self.needs_redraw = false;
    }

    /// Fold one event into state and return effects for the
    /// caller to apply to hardware.
    pub fn handle_event(&mut self, event: &SystemEvent, now: Instant) -> Effects {
        let mut out: Effects = Vec::new();

        // 1. Snapshot events: update the cached fields.
        self.apply_snapshot(event, &mut out);

        // 2. Non-user wake sources: wake the device, then let the
        // event continue through to screen dispatch (except WoM,
        // which just wakes).
        if self.sleeping && events::is_wake_source(event) {
            self.wake(now, &mut out);
            if matches!(event, SystemEvent::WakeOnMotion) {
                return out;
            }
        }

        // 3. User activity: resets idle timer; wakes the device or
        // (if BOOT while awake) triggers a "sleep now" shortcut.
        if events::is_user_activity(event) {
            self.last_activity = now;
            if self.sleeping {
                self.wake(now, &mut out);
                return out; // consume the event so accidental
                            // taps/swipes on wake don't dispatch
                            // to the screen.
            }
            if matches!(event, SystemEvent::BootButtonPressed) {
                let _ = out.push(Effect::MotorPulse { duration_ms: 100 });
                self.sleep(&mut out);
                return out;
            }
        }

        // From here on we only dispatch to the screen when awake.
        if self.sleeping {
            return out;
        }

        // 4. System-level swipe-down-from-top opens the panel.
        // Push the pre-panel screen so `Action::Back` from an app
        // launched via the panel returns here, not hardcoded
        // Clock.
        if !matches!(self.screen.id(), ScreenId::Panel) {
            if let SystemEvent::Swipe { dir: SwipeDir::Down, region: SwipeRegion::Top } = event {
                let previous = self.screen.id();
                self.nav_stack.push(previous);
                self.screen.open_panel(previous, &self.cached_data);
                self.needs_redraw = true;
                return out;
            }
        }

        // 5. Forward to the active screen and dispatch its Action.
        let action = self.screen.on_event(event, &mut self.cached_data);
        self.dispatch_action(event, action, &mut out);
        out
    }

    /// Time-driven advance: buzz-pattern tick, dim/idle-sleep
    /// checks. Call once per loop iteration.
    pub fn tick(&mut self, now: Instant) -> Effects {
        let mut out: Effects = Vec::new();
        self.tick_buzz(now, &mut out);
        self.apply_dim_state(now, &mut out);
        self.check_idle_sleep(now, &mut out);
        out
    }

    // --- internals -----------------------------------------------------------

    /// Update cached snapshot fields from snapshot-carrying events.
    /// Also handles the TimerExpired / AlarmFired screen switches.
    fn apply_snapshot(&mut self, event: &SystemEvent, out: &mut Effects) {
        match event {
            SystemEvent::TimeUpdated { data } => {
                self.cached_data.time = *data;
                // Check if the next alarm needs reprogramming.
                let t = &self.cached_data.time;
                let weekday = crate::ui::screens::alarm::day_of_week(
                    t.year as i32, t.month as i32, t.day as i32,
                );
                match self.cached_data.alarms.plan_reprogram(t.hour, t.minute, weekday) {
                    None => {}
                    Some(AlarmReprogram::SetAlarm { hour, minute }) => {
                        let _ = out.push(Effect::SetAlarm { hour, minute, weekday: None });
                    }
                    Some(AlarmReprogram::CancelAlarm) => {
                        let _ = out.push(Effect::CancelAlarm);
                    }
                }
            }
            SystemEvent::PowerUpdated { data } => {
                self.cached_data.power = *data;
            }
            SystemEvent::MotionUpdated { data } => {
                self.cached_data.motion = *data;
            }
            SystemEvent::TimerExpired => {
                self.cached_data.timer = TimerState::Idle { duration: Duration::from_ticks(0) };
                if !matches!(self.screen.id(), ScreenId::Timer) {
                    self.nav_stack.push(self.screen.id());
                    self.screen.switch_to(ScreenId::Timer, &self.cached_data);
                }
                self.needs_redraw = true;
            }
            SystemEvent::AlarmFired => {
                if !matches!(self.screen.id(), ScreenId::Alarm) {
                    self.nav_stack.push(self.screen.id());
                    self.screen.switch_to(ScreenId::Alarm, &self.cached_data);
                }
                self.needs_redraw = true;
            }
            SystemEvent::SelfTestUpdated { id, result } => {
                let idx = *id as usize;
                if idx < NUM_SELF_TESTS {
                    self.cached_data.self_tests[idx] = *result;
                }
                self.needs_redraw = true;
            }
            SystemEvent::TouchPressed { x, y } => {
                self.cached_data.touch = TouchData { x: Some(*x), y: Some(*y) };
            }
            SystemEvent::TouchReleased => {
                self.cached_data.touch = TouchData::default();
            }
            SystemEvent::HalfMinuteChanged | SystemEvent::BatteryChanged { .. } => {
                self.needs_redraw = true;
            }
            _ => {}
        }
    }

    /// Dispatch a screen-returned `Action` into state mutations
    /// and effects.
    fn dispatch_action(&mut self, event: &SystemEvent, action: Action, out: &mut Effects) {
        match action {
            Action::None => {
                // Home-row nav fallback: content L/R swipes cycle
                // through home-row apps (not when already on
                // Panel).
                if !matches!(self.screen.id(), ScreenId::Panel) {
                    if let SystemEvent::Swipe { dir, region: SwipeRegion::Content } = event {
                        match dir {
                            SwipeDir::Right => {
                                let next = screens::cycle_home_app(self.screen.id(), true);
                                self.screen.switch_to(next, &self.cached_data);
                                self.needs_redraw = true;
                            }
                            SwipeDir::Left => {
                                let prev = screens::cycle_home_app(self.screen.id(), false);
                                self.screen.switch_to(prev, &self.cached_data);
                                self.needs_redraw = true;
                            }
                            _ => {}
                        }
                    }
                }
            }
            Action::Redraw => self.needs_redraw = true,
            Action::SwitchScreen(id) => {
                // Modal replace-top: when leaving Panel the
                // pre-panel screen is already on the nav stack.
                if !matches!(self.screen.id(), ScreenId::Panel) {
                    self.nav_stack.push(self.screen.id());
                }
                self.screen.switch_to(id, &self.cached_data);
                self.needs_redraw = true;
            }
            Action::Back => {
                let target = self.nav_stack.pop_or_home();
                self.screen.switch_to(target, &self.cached_data);
                self.needs_redraw = true;
            }
            Action::Shutdown => {
                let _ = out.push(Effect::Shutdown);
            }
            Action::RunSelfTest(id) => {
                let _ = out.push(Effect::RunSelfTest(id));
                self.needs_redraw = true;
            }
            Action::StartTimer { seconds } => {
                let _ = out.push(Effect::StartTimer { seconds });
                self.needs_redraw = true;
            }
            Action::CancelTimer => {
                let _ = out.push(Effect::CancelTimer);
                self.needs_redraw = true;
            }
            Action::SetAlarm { hour, minute, weekday } => {
                let _ = out.push(Effect::SetAlarm { hour, minute, weekday });
                self.needs_redraw = true;
            }
            Action::CancelAlarm => {
                let _ = out.push(Effect::CancelAlarm);
                self.needs_redraw = true;
            }
            Action::SetTime { year, month, day, hour, minute, second } => {
                let _ = out.push(Effect::SetTime { year, month, day, hour, minute, second });
                self.needs_redraw = true;
            }
            Action::StartBuzz { on_ms, off_ms } => {
                self.buzz = Some(BuzzPattern::start(
                    on_ms as u64,
                    off_ms as u64,
                    self.last_activity, // any Instant; tick() will
                                        // re-anchor on the first
                                        // call.
                ));
                let _ = out.push(Effect::MotorOn);
            }
            Action::StopBuzz => {
                self.buzz = None;
                let _ = out.push(Effect::MotorOff);
                self.needs_redraw = true;
            }
            Action::DismissAlarm => {
                self.buzz = None;
                let _ = out.push(Effect::MotorOff);
                let target = self.nav_stack.pop_or_home();
                self.screen.switch_to(target, &self.cached_data);
                self.needs_redraw = true;
            }
            Action::SnoozeAlarm => {
                self.buzz = None;
                let _ = out.push(Effect::MotorOff);
                self.cached_data.alarms.snoozed = true;
                let t = &self.cached_data.time;
                let (hour, minute) = AlarmState::compute_snooze(t.hour, t.minute, 10);
                let _ = out.push(Effect::SetAlarm { hour, minute, weekday: None });
                let target = self.nav_stack.pop_or_home();
                self.screen.switch_to(target, &self.cached_data);
                self.needs_redraw = true;
            }
        }
    }

    /// Enter low-power sleep. Idempotent. Queues the display-Off
    /// transition + SLEEP_WATCH broadcast; the main loop picks
    /// these up and the *next* tick enters hardware light sleep
    /// via [`Self::tick`] -> [`Effect::EnterLightSleep`].
    fn sleep(&mut self, out: &mut Effects) {
        if self.sleeping {
            return;
        }
        self.sleeping = true;
        let _ = out.push(Effect::BroadcastSleeping);
        let _ = out.push(Effect::TransitionDisplay {
            from: self.display_state,
            to: DisplayState::Off,
        });
        self.display_state = DisplayState::Off;
    }

    /// Exit low-power sleep. Idempotent.
    fn wake(&mut self, now: Instant, out: &mut Effects) {
        if !self.sleeping {
            return;
        }
        self.sleeping = false;
        self.last_activity = now;
        let _ = out.push(Effect::BroadcastAwake);
        let _ = out.push(Effect::TransitionDisplay {
            from: self.display_state,
            to: DisplayState::Active,
        });
        self.display_state = DisplayState::Active;
        self.needs_redraw = true;
    }

    /// Advance the buzz pattern. Emits [`Effect::MotorOn`] /
    /// [`Effect::MotorOff`] when the phase flips.
    fn tick_buzz(&mut self, now: Instant, out: &mut Effects) {
        let Some(pattern) = self.buzz.as_mut() else {
            return;
        };
        match pattern.tick(now) {
            BuzzAction::None => {}
            BuzzAction::TurnOn => { let _ = out.push(Effect::MotorOn); }
            BuzzAction::TurnOff => { let _ = out.push(Effect::MotorOff); }
        }
    }

    /// Apply the Active <-> Dim transition when awake. No-op when
    /// sleeping (display is Off and [`Self::sleep`] / [`Self::wake`]
    /// handle that).
    fn apply_dim_state(&mut self, now: Instant, out: &mut Effects) {
        if self.sleeping {
            return;
        }
        let idle = now.duration_since(self.last_activity);
        let target = if idle >= Duration::from_secs(self.config.display.dim_timeout_s) {
            DisplayState::Dim
        } else {
            DisplayState::Active
        };
        if target != self.display_state {
            let _ = out.push(Effect::TransitionDisplay {
                from: self.display_state,
                to: target,
            });
            self.display_state = target;
        }
    }

    /// Trigger sleep if the idle timer has expired. No-op if
    /// already sleeping.
    fn check_idle_sleep(&mut self, now: Instant, out: &mut Effects) {
        if self.sleeping {
            return;
        }
        let idle = now.duration_since(self.last_activity);
        if idle >= Duration::from_secs(self.config.display.off_timeout_s) {
            self.sleep(out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> Model {
        Model::new(
            SystemData::default(),
            Config::default(),
            Instant::from_millis(0),
        )
    }

    #[test]
    fn boot_button_while_awake_sleeps_with_buzz_pulse() {
        let mut m = fresh();
        let fx = m.handle_event(&SystemEvent::BootButtonPressed, Instant::from_millis(0));
        assert!(m.sleeping);
        // First effect: the BOOT haptic pulse.
        assert_eq!(fx[0], Effect::MotorPulse { duration_ms: 100 });
        // Then the sleep transitions.
        assert!(fx.contains(&Effect::BroadcastSleeping));
        assert!(fx.iter().any(|e| matches!(
            e,
            Effect::TransitionDisplay { to: DisplayState::Off, .. }
        )));
    }

    #[test]
    fn touch_while_sleeping_wakes_and_consumes_event() {
        let mut m = fresh();
        // Go to sleep first (as if BOOT was pressed).
        m.handle_event(&SystemEvent::BootButtonPressed, Instant::from_millis(0));
        assert!(m.sleeping);

        // Touch wakes us and does NOT dispatch to the screen.
        let fx = m.handle_event(
            &SystemEvent::TouchPressed { x: 100, y: 100 },
            Instant::from_millis(5_000),
        );
        assert!(!m.sleeping);
        assert!(fx.contains(&Effect::BroadcastAwake));
        assert!(fx.iter().any(|e| matches!(
            e,
            Effect::TransitionDisplay { to: DisplayState::Active, .. }
        )));
    }

    #[test]
    fn shutdown_action_produces_effect() {
        let mut m = fresh();
        // Poke directly via dispatch_action - bypasses screen.
        let mut out: Effects = Vec::new();
        m.dispatch_action(&SystemEvent::BootButtonPressed, Action::Shutdown, &mut out);
        assert_eq!(out[0], Effect::Shutdown);
    }

    #[test]
    fn snooze_emits_motor_off_and_set_alarm_at_now_plus_10() {
        let mut m = fresh();
        m.cached_data.time.hour = 7;
        m.cached_data.time.minute = 55;
        let mut out: Effects = Vec::new();
        m.dispatch_action(&SystemEvent::BootButtonPressed, Action::SnoozeAlarm, &mut out);
        assert!(out.contains(&Effect::MotorOff));
        assert!(out.contains(&Effect::SetAlarm { hour: 8, minute: 5, weekday: None }));
        assert!(m.cached_data.alarms.snoozed);
    }

    #[test]
    fn idle_past_dim_threshold_emits_dim_transition() {
        let mut m = fresh();
        // dim_timeout_s defaults; just step well past it.
        let dim_timeout = m.config.display.dim_timeout_s;
        let fx = m.tick(Instant::from_millis((dim_timeout as u64 + 1) * 1000));
        assert!(fx.iter().any(|e| matches!(
            e,
            Effect::TransitionDisplay { to: DisplayState::Dim, .. }
        )));
        assert_eq!(m.display_state, DisplayState::Dim);
    }

    #[test]
    fn idle_past_off_threshold_enters_sleep() {
        let mut m = fresh();
        let off_timeout = m.config.display.off_timeout_s;
        let fx = m.tick(Instant::from_millis((off_timeout as u64 + 1) * 1000));
        assert!(m.sleeping);
        assert!(fx.contains(&Effect::BroadcastSleeping));
    }
}
