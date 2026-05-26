//! Command and broadcast payload types for the firmware task
//! channels.
//!
//! These enums used to live in `firmware::system::bus` alongside
//! the `Signal`/`Watch` statics that carry them. The payload types
//! themselves are pure value enums with no hardware coupling, so
//! they moved here to let [`crate::model::Effect`] carry them
//! directly. The statics (`RTC_COMMAND`, `IMU_COMMAND`,
//! `SLEEP_WATCH`) still live on the firmware side and re-export
//! these via `pub use`.

use crate::events::SelfTestId;

/// Broadcast on the SLEEP_WATCH `Watch` so subscribers (IMU task,
/// touch task, power task) can flip between awake and low-power
/// modes in step with the main loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SleepState {
    /// System is awake - full sensor polling, display active.
    Awake,
    /// System is sleeping - peripherals should switch to their
    /// lowest-power modes (e.g. IMU into WoM, idle tick stopped).
    Sleeping,
}

/// Main-loop -> audio task commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioCommand {
    /// Start the repeating alarm / timer alert tone. The audio task
    /// brings the speaker up lazily on the first `PlayAlarm` (the
    /// codec stays dormant until then to save idle current) and loops
    /// the tone until a `Stop` arrives.
    PlayAlarm,
    /// Silence the alert tone. Mutes the speaker amplifier; the codec
    /// is left warm so a re-fire (e.g. snooze) doesn't pay the
    /// bring-up latency again.
    Stop,
}

/// Main-loop -> IMU task commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImuCommand {
    /// Run one specific hardware self-test, identified by its
    /// [`SelfTestId`]. Only IMU-owned test ids make sense here;
    /// unrecognised variants are logged and ignored by the task.
    RunSelfTest(SelfTestId),
}

/// Main-loop -> RTC task commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RtcCommand {
    /// Start the hardware countdown timer. The RTC task picks the
    /// best clock source (Hz1 for <= 255s, Per60 for longer) and
    /// calls `Rtc::set_timer`. When the countdown expires, the
    /// task emits `SystemEvent::TimerExpired`.
    StartTimer { seconds: u32 },
    /// Cancel a running hardware countdown timer.
    CancelTimer,
    /// Set an alarm at the given hour and minute. Optionally
    /// restrict to a single weekday (0=Sunday..6=Saturday); `None`
    /// fires every day. The RTC task calls `Rtc::set_alarm` with
    /// second=0. When the alarm fires, the task emits
    /// `SystemEvent::AlarmFired`.
    SetAlarm { hour: u8, minute: u8, weekday: Option<u8> },
    /// Cancel a set alarm.
    CancelAlarm,
    /// Set the RTC date and time. Used by the settings time screen.
    #[allow(dead_code)]
    SetTime { year: u16, month: u8, day: u8, hour: u8, minute: u8, second: u8 },
    /// Change the time poll interval (in seconds). Affects how
    /// often `TimeUpdated` events are emitted. Configurable from
    /// settings.
    #[allow(dead_code)]
    SetTimePollInterval { seconds: u8 },
    /// Check the RTC for a latched alarm / timer flag right now and
    /// emit the matching `AlarmFired` / `TimerExpired` if one is set.
    /// This is how alarm/timer expiry is detected on boards with no
    /// RTC INT line (e.g. the C6): embassy timers - including the RTC
    /// task's own software poll - pause across light sleep, so the
    /// manager signals this on each heartbeat wake to catch expiry
    /// that fired while the device was asleep.
    Poll,
}
