//! System-wide plumbing for the event-driven architecture.
//!
//! This module owns the globally-accessible statics that peripheral
//! tasks share with the main loop:
//!
//!   * [`EVENTS`]  - MPSC event channel. All tasks push events into
//!     this channel; the main loop drains it.
//!   * [`I2C_BUS`] - Shared I2C bus protected by an async mutex.
//!     Every task that needs I2C locks this before accessing it.
//!   * [`SLEEP_SIGNAL`] - Signal that broadcasts the current sleep
//!     state. Tasks can `.wait()` on it to react to sleep/wake
//!     transitions (e.g. the IMU task switches into WoM mode on
//!     entering sleep).
//!
//! Everything here is initialised in `main()` and then referenced
//! by tasks via `&'static` references, so lifetimes work out for
//! `#[embassy_executor::task]` definitions.

use crate::events::SystemEvent;
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::Channel,
    mutex::Mutex,
    signal::Signal,
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

/// Broadcast signal for sleep state changes.
///
/// Main publishes the new state via `SLEEP_SIGNAL.signal(state)`
/// when entering/exiting sleep. Tasks that care can
/// `SLEEP_SIGNAL.wait().await` to react.
///
/// **Single-consumer limit**: `Signal::wait()` consumes the value,
/// so only one task can subscribe. Today that's the IMU task.
/// Before adding a second subscriber (touch monitor mode, RTC
/// HMI gating, stretched power poll, ...), swap this for an
/// `embassy_sync::pubsub::PubSubChannel` or split into one signal
/// per task, otherwise the IMU will silently stop getting wake
/// transitions.
pub static SLEEP_SIGNAL: Signal<CriticalSectionRawMutex, SleepState> = Signal::new();

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
