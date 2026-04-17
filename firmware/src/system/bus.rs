//! System-wide plumbing for the event-driven architecture.
//!
//! This module owns the globally-accessible statics that peripheral
//! tasks share with the main loop:
//!
//!   * [`EVENTS`]  - MPSC event channel. All tasks push events into
//!     this channel; the main loop drains it.
//!   * [`I2C_BUS`] - Shared I2C bus protected by an async mutex.
//!     Every task that needs I2C locks this before accessing it.
//!   * [`SLEEP_WATCH`] - Watch that broadcasts the current sleep
//!     state. Tasks subscribe once at startup and await `changed()`
//!     in their main loops to react to sleep/wake transitions
//!     (IMU swaps between snapshot polling and WoM, touch flips
//!     between Active and Monitor power modes, the power task
//!     stretches its PMU poll cadence).
//!
//! Everything here is initialised in `main()` and then referenced
//! by tasks via `&'static` references, so lifetimes work out for
//! `#[embassy_executor::task]` definitions.

use crate::events::{SelfTestId, SystemEvent};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::Channel,
    mutex::Mutex,
    signal::Signal,
    watch::Watch,
};
use esp_hal::{i2c::master::I2c, Blocking};
use static_cell::StaticCell;

/// Size of the system event channel. Should be large enough to
/// buffer a burst of events without blocking producers but small
/// enough that a backed-up main loop gets noticed.
pub const EVENT_CHANNEL_SIZE: usize = 32;

/// Global MPSC event channel.
///
/// All peripheral tasks push [`SystemEvent`]s into this channel.
/// The main loop drains it via `EVENTS.receive().await`.
pub static EVENTS: Channel<CriticalSectionRawMutex, SystemEvent, EVENT_CHANNEL_SIZE> =
    Channel::new();

/// Current system sleep state, used to coordinate peripheral tasks
/// with the main loop's sleep/wake transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SleepState {
    /// System is awake - full sensor polling, display active.
    Awake,
    /// System is sleeping - peripherals should switch to their
    /// lowest-power modes (e.g. IMU into WoM, idle tick stopped).
    Sleeping,
}

/// Maximum number of tasks that can subscribe to [`SLEEP_WATCH`] at
/// once. Bump this when adding a new subscriber beyond the current
/// three (IMU, touch, power). Each subscriber consumes one slot
/// whether or not it's currently parked on `changed()`.
pub const SLEEP_WATCH_SUBSCRIBERS: usize = 4;

/// Broadcast of the current system sleep state.
///
/// Main publishes transitions via
/// `SLEEP_WATCH.sender().send(state)` when entering or exiting
/// sleep. Any task that needs to react acquires a receiver once
/// at task startup (`SLEEP_WATCH.receiver().unwrap()`) and awaits
/// `rx.changed()` inside its main loop's select.
///
/// The `Watch` primitive fans out the latest value to multiple
/// independent receivers, each tracking its own "last seen" id -
/// exactly what's needed for sleep state broadcast without the
/// single-consumer limitation of the old `Signal`-based design.
/// Current subscribers: IMU task (enters WoM mode on Sleeping,
/// restores normal config on Awake), touch task (switches
/// FT3168 to Monitor mode / back to Active), power task (slows
/// its PMU poll cadence while sleeping).
pub static SLEEP_WATCH: Watch<CriticalSectionRawMutex, SleepState, SLEEP_WATCH_SUBSCRIBERS> =
    Watch::new();

/// Command sent from the main loop to the IMU task.
///
/// Wraps imperative actions the task can't initiate on its own -
/// typically because the request originates from UI input that the
/// main loop receives first. The enum exists (rather than the signal
/// carrying a primitive payload directly) because more commands are
/// expected: recalibrate gyro bias on demand, force a WoM re-arm,
/// switch scale/ODR at runtime, etc. Adding a new command is just a
/// new variant plus a match arm in the IMU task's command handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImuCommand {
    /// Run one specific hardware self-test, identified by its
    /// [`SelfTestId`]. Only IMU-owned test ids make sense here;
    /// unrecognised variants are logged and ignored by the task.
    RunSelfTest(SelfTestId),
}

/// Main-to-IMU command signal.
///
/// The main loop publishes an [`ImuCommand`] here when a UI screen
/// returns an action that needs IMU hardware access (e.g. tapping a
/// self-test card returns `Action::RunSelfTest(id)`, the main loop
/// routes it here). The IMU task listens for it as one arm of its
/// awake-branch select.
///
/// Single-consumer: only the IMU task should call `wait()` on this.
pub static IMU_COMMAND: Signal<CriticalSectionRawMutex, ImuCommand> = Signal::new();

/// Command sent from the main loop to the RTC task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RtcCommand {
    /// Start the hardware countdown timer. The RTC task picks the
    /// best clock source (Hz1 for <= 255s, Per60 for longer) and
    /// calls `Rtc::set_timer`. When the countdown expires, the
    /// task emits `SystemEvent::TimerExpired`.
    StartTimer { seconds: u32 },
    /// Cancel a running hardware countdown timer.
    CancelTimer,
    /// Set an alarm at the given hour and minute. Optionally restrict
    /// to a single weekday (0=Sunday..6=Saturday); `None` fires every
    /// day. The RTC task calls `Rtc::set_alarm` with second=0.
    /// When the alarm fires, the task emits `SystemEvent::AlarmFired`.
    SetAlarm { hour: u8, minute: u8, weekday: Option<u8> },
    /// Cancel a set alarm.
    CancelAlarm,
    /// Set the RTC date and time. Used by the settings time screen.
    #[allow(dead_code)]
    SetTime { year: u16, month: u8, day: u8, hour: u8, minute: u8, second: u8 },
    /// Change the time poll interval (in seconds). Affects how often
    /// `TimeUpdated` events are emitted. Configurable from settings.
    #[allow(dead_code)]
    SetTimePollInterval { seconds: u8 },
}

/// Main-to-RTC command signal.
///
/// Single-consumer: only the RTC task should call `wait()` on this.
pub static RTC_COMMAND: Signal<CriticalSectionRawMutex, RtcCommand> = Signal::new();

/// Type alias for the shared I2C bus, protected by an async mutex.
///
/// Four devices sit on this bus (PMU, touch, RTC, IMU) and each
/// lives in its own task. Tasks lock the mutex before reading or
/// writing, which serializes access without requiring any
/// per-device coordination.
pub type SharedI2c = Mutex<CriticalSectionRawMutex, I2c<'static, Blocking>>;

/// One-time storage for the shared I2C bus. Initialised in
/// `SystemManager::init` and handed to tasks as `&'static SharedI2c`.
pub static I2C_BUS: StaticCell<SharedI2c> = StaticCell::new();
