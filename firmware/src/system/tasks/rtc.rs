//! RTC (PCF85063A) task state.
//!
//! Owns the RTC driver plus the INT# line (GPIO39). The PCF85063
//! INT# pin is shared across three live sources in this firmware:
//!
//!   * Alarm match - latches AF in Control_2, drives `AlarmFired`
//!   * Countdown timer expiry - latches TF, drives `TimerExpired`
//!   * CLKOUT (unused here)
//!
//! HMI (half-minute interrupt) and MI (minute interrupt) are *not*
//! enabled - the PCF85063 has a silicon quirk where TF latches
//! prematurely (~20 s into a 60 s countdown) when HMI is active
//! on the shared INT# line. We drive clock-face redraws from a
//! 1 s software poll in the task loop instead (`TimeUpdated`), so
//! no user-visible tick depends on INT# pulses.
//!
//! The task waits on the falling edge, reads Control_2 to find
//! out which source fired, clears any latched flags, and emits
//! the corresponding system event. An INT# with neither AF nor
//! TF set is unexpected (nothing else should be able to assert
//! INT#) and gets logged as a warn.
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
use crate::system::bus::{EVENTS, RTC_COMMAND, RtcCommand, SharedI2c};
use drivers::rtc::{Alarm, Rtc, Config as RtcConfig, DateTime as RtcDateTime, TimerClock, TimerOutput};
use embassy_futures::select::{select3, Either3};
use embassy_time::{Duration, Timer};
use embedded_hal::i2c::I2c as I2cTrait;
use esp_hal::gpio::Input;

/// RTC task: wait on INT#, classify the source (alarm / timer),
/// emit the matching event and a fresh `TimeUpdated` snapshot so
/// the main loop's cached time stays current.
///
/// Default time poll interval in seconds.
const DEFAULT_TIME_POLL_SECS: u64 = 1;

#[embassy_executor::task]
pub async fn rtc_task(bus: &'static SharedI2c, mut state: RtcTaskState<'static>) {
    loop {
        let poll = Duration::from_secs(state.poll_secs);
        match select3(
            state.wait_for_int(),
            RTC_COMMAND.wait(),
            Timer::after(poll),
        ).await {
            Either3::First(()) => {
                // INT# fired (timer expiry or alarm).
                let (event, time) = {
                    let mut i2c = bus.lock().await;
                    let event = state.classify_interrupt(&mut *i2c);
                    let time = state.snapshot(&mut *i2c);
                    (event, time)
                };
                EVENTS.send(SystemEvent::TimeUpdated { data: time }).await;
                if let Some(ev) = event {
                    EVENTS.send(ev).await;
                }
            }
            Either3::Second(cmd) => {
                let mut i2c = bus.lock().await;
                state.handle_command(&mut *i2c, cmd);
            }
            Either3::Third(()) => {
                // Software poll: read RTC time and send update.
                let time = {
                    let mut i2c = bus.lock().await;
                    state.snapshot(&mut *i2c)
                };
                EVENTS.send(SystemEvent::TimeUpdated { data: time }).await;
            }
        }
    }
}

// `TimeData` (struct + Default + From<&RtcDateTime>) lives in
// `app_core::data`. Re-exported so existing `crate::system::tasks::
// rtc::TimeData` imports in firmware keep resolving.
pub use app_core::data::TimeData;

pub struct RtcTaskState<'d> {
    pub rtc: Rtc,
    int_pin: Input<'d>,
    /// How often to poll the RTC for time updates (in seconds).
    poll_secs: u64,
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
                    // Y2K midnight - the PCF85063's epoch.
                    let default_time = RtcDateTime::new(2000, 1, 1, 0, 0, 0, 0);
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

        // HMI/MI stay disabled - the PCF85063 latches TF prematurely
        // when HMI is active on the shared INT# line. Clock redraws
        // are driven by the 1 s software poll in `rtc_task` instead.

        Self { rtc, int_pin, poll_secs: DEFAULT_TIME_POLL_SECS }
    }

    /// Read the current date/time from the RTC. Called by the
    /// render path to build a fresh SystemData snapshot.
    #[allow(dead_code)]
    pub fn read_time(&self, i2c: &mut impl I2cTrait) -> Option<RtcDateTime> {
        self.rtc.get(i2c).ok()
    }

    /// Build a `TimeData` snapshot from the current RTC reading.
    /// Returns the `Default` (epoch-ish) values if the I2C read
    /// fails.
    pub fn snapshot(&self, i2c: &mut impl I2cTrait) -> TimeData {
        self.rtc.get(i2c).ok().as_ref().map(TimeData::from).unwrap_or_default()
    }

    /// Handle a command from the main loop.
    pub fn handle_command(&mut self, i2c: &mut impl I2cTrait, cmd: RtcCommand) {
        match cmd {
            RtcCommand::StartTimer { seconds } => {
                if seconds == 0 {
                    log::warn!("RTC: timer request with 0 seconds, ignoring");
                    return;
                }
                let (clock, value) = if seconds <= 255 {
                    (TimerClock::Hz1, seconds as u8)
                } else {
                    let ticks = ((seconds + 59) / 60).min(255) as u8;
                    (TimerClock::Per60, ticks)
                };
                match self.rtc.set_timer(i2c, value, clock, TimerOutput::Pulse) {
                    Ok(()) => log::info!("RTC: timer started, value={} clock={:?}", value, clock),
                    Err(_) => log::error!("RTC: failed to start timer"),
                }
            }
            RtcCommand::CancelTimer => {
                match self.rtc.disable_timer(i2c) {
                    Ok(()) => log::info!("RTC: timer cancelled"),
                    Err(_) => log::error!("RTC: failed to cancel timer"),
                }
            }
            RtcCommand::SetAlarm { hour, minute, weekday } => {
                let alarm = Alarm {
                    second: Some(0),
                    minute: Some(minute),
                    hour: Some(hour),
                    day: None,
                    weekday,
                };
                match self.rtc.set_alarm(i2c, &alarm) {
                    Ok(()) => log::info!("RTC: alarm set {:02}:{:02} weekday={:?}", hour, minute, weekday),
                    Err(_) => log::error!("RTC: failed to set alarm"),
                }
            }
            RtcCommand::CancelAlarm => {
                match self.rtc.disable_alarm(i2c) {
                    Ok(()) => log::info!("RTC: alarm cancelled"),
                    Err(_) => log::error!("RTC: failed to cancel alarm"),
                }
            }
            RtcCommand::SetTime { year, month, day, hour, minute, second } => {
                let dt = RtcDateTime::new(year, month, day, 0, hour, minute, second);
                match self.rtc.set(i2c, &dt) {
                    Ok(()) => log::info!("RTC: time set {:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                        year, month, day, hour, minute, second),
                    Err(_) => log::error!("RTC: failed to set time"),
                }
            }
            RtcCommand::SetTimePollInterval { seconds } => {
                self.poll_secs = seconds.max(1) as u64;
                log::info!("RTC: poll interval set to {}s", self.poll_secs);
            }
        }
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
            let _ = self.rtc.disable_timer(i2c);
            Some(SystemEvent::TimerExpired)
        } else {
            // Unexpected: HMI/MI are never enabled, so INT# should
            // only fire for alarm or timer. Log and swallow.
            log::warn!("RTC: INT# fired with no AF/TF set - stray pulse");
            None
        }
    }
}
