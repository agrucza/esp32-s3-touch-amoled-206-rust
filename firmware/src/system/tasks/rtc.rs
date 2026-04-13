//! RTC (PCF85063A) task state.
//!
//! Owns the RTC driver plus the INT# line (GPIO39). The PCF85063
//! INT# pin is shared across four sources:
//!
//!   * Half-minute interrupt (HMI) - pulses low at second=0 and
//!     second=30, drives `HalfMinuteChanged`
//!   * Alarm match - latches AF in Control_2, drives `AlarmFired`
//!   * Countdown timer expiry - latches TF, drives `TimerExpired`
//!   * CLKOUT (unused here)
//!
//! The task waits on the falling edge, reads Control_2 to find
//! out which source fired, clears any latched flags, and emits
//! the corresponding system event. HMI pulses are unlatched -
//! if neither AF nor TF is set when the INT fires, we know it
//! was the periodic half-minute tick.
//!
//! ## Phase 4 task loop sketch
//!
//! ```ignore
//! #[embassy_executor::task]
//! async fn rtc_task(bus: &'static SharedI2c, mut state: RtcTaskState<'static>) {
//!     loop {
//!         state.wait_for_int().await;
//!         let mut i2c = bus.lock().await;
//!         if let Some(event) = state.classify_interrupt(&mut *i2c) {
//!             drop(i2c);
//!             EVENTS.send(event).await;
//!         }
//!     }
//! }
//! ```

use crate::events::SystemEvent;
use drivers::rtc::{Rtc, Config as RtcConfig, DateTime as RtcDateTime};
use embedded_hal::i2c::I2c as I2cTrait;
use esp_hal::gpio::Input;

/// Calendar time of day, consumed by clock-style screens.
/// Defaults to an arbitrary recent date so screens have
/// something reasonable to render before the first RTC read.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // `second` is read by future screens (e.g. a seconds face)
pub struct TimeData {
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

impl Default for TimeData {
    fn default() -> Self {
        Self { hour: 0, minute: 0, second: 0, year: 2026, month: 1, day: 1 }
    }
}

impl From<&RtcDateTime> for TimeData {
    fn from(dt: &RtcDateTime) -> Self {
        Self {
            hour: dt.hour,
            minute: dt.minute,
            second: dt.second,
            year: dt.year,
            month: dt.month,
            day: dt.day,
        }
    }
}

pub struct RtcTaskState<'d> {
    pub rtc: Rtc,
    int_pin: Input<'d>,
}

impl<'d> RtcTaskState<'d> {
    /// Initialize the RTC, set a default time if the oscillator
    /// stopped, and enable the minute interrupt so GPIO39 pulses
    /// at second=0.
    pub fn init(int_pin: Input<'d>, i2c: &mut impl I2cTrait) -> Self {
        log::info!("RTC: initializing PCF85063...");
        let rtc = Rtc::new(RtcConfig::default());
        match rtc.init(i2c) {
            Err(_) => log::error!("RTC: device not found on I2C bus"),
            Ok(os_flag) => {
                if os_flag {
                    log::warn!("RTC: oscillator-stop flag set - time is invalid");
                } else {
                    log::info!("RTC: oscillator running, time is valid");
                }

                let needs_set = os_flag || match rtc.get(i2c) {
                    Ok(ref dt) => !dt.is_valid(),
                    Err(_) => true,
                };

                if needs_set {
                    log::warn!("RTC: time invalid - setting default");
                    let default_time = RtcDateTime::new(2026, 3, 30, 0, 12, 0, 0);
                    if rtc.set(i2c, &default_time).is_err() {
                        log::error!("RTC: failed to set time");
                    }
                }

                match rtc.get(i2c) {
                    Ok(dt) => log::info!("RTC: {:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                        dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second),
                    Err(_) => log::error!("RTC: failed to read time"),
                }
            }
        }

        if rtc.enable_half_minute_interrupt(i2c).is_err() {
            log::warn!("RTC: failed to enable half-minute interrupt");
        }

        Self { rtc, int_pin }
    }

    /// Read the current date/time from the RTC. Called by the
    /// render path to build a fresh SystemData snapshot.
    pub fn read_time(&self, i2c: &mut impl I2cTrait) -> Option<RtcDateTime> {
        self.rtc.get(i2c).ok()
    }

    /// Build a `TimeData` snapshot from the current RTC reading.
    /// Returns the `Default` (epoch-ish) values if the I2C read
    /// fails.
    pub fn snapshot(&self, i2c: &mut impl I2cTrait) -> TimeData {
        self.rtc.get(i2c).ok().as_ref().map(TimeData::from).unwrap_or_default()
    }

    /// Async wait for any source on the INT# line (GPIO39 falling
    /// edge). This could be the half-minute tick, a fired alarm,
    /// or a timer expiry - call `classify_interrupt` afterwards
    /// to determine which.
    pub async fn wait_for_int(&mut self) {
        self.int_pin.wait_for_falling_edge().await;
    }

    /// Read Control_2 to figure out which INT source fired,
    /// clear any latched flags (AF/TF), and return the matching
    /// system event to emit. The half-minute interrupt doesn't
    /// latch a flag - if neither alarm nor timer is set we assume
    /// it was the HMI pulse.
    ///
    /// Returns `None` only on I2C failure, which shouldn't happen
    /// in practice.
    pub fn classify_interrupt(&self, i2c: &mut impl I2cTrait) -> Option<SystemEvent> {
        let status = self.rtc.read_status(i2c).ok()?;
        if status.alarm_flag {
            let _ = self.rtc.clear_alarm_flag(i2c);
            Some(SystemEvent::AlarmFired)
        } else if status.timer_flag {
            let _ = self.rtc.clear_timer_flag(i2c);
            Some(SystemEvent::TimerExpired)
        } else {
            // Unlatched HMI pulse - second=0 or second=30.
            Some(SystemEvent::HalfMinuteChanged)
        }
    }
}
