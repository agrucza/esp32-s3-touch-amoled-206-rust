//! Power subsystem GPIO controls.
//!
//! Holds the non-I2C power hardware that the main task needs
//! direct synchronous access to:
//!
//!   * **SYS_OUT latch** (GPIO10) - holds the board power rail
//!     on; released on `shutdown()` to power down.
//!   * **Motor** (GPIO18) - haptic feedback for button presses.
//!
//! The I2C side of the PMU (AXP2101 register access, battery
//! readings, interrupt polling) lives in
//! `system::tasks::power::PowerTaskState`. Initialization of
//! the PMU itself happens here in `PowerControls::init` since
//! the rails must be enabled before any other I2C subsystem
//! can be used, and the caller receives a `Pmu` handle that
//! then gets wrapped in `PowerTaskState`.

use drivers::pmu::{Config as PmuConfig, Pmu};
use embedded_hal::i2c::I2c;
use esp_hal::gpio::Output;

/// Snapshot of power-related readings at one point in time.
/// Produced by `PowerTaskState::snapshot`; consumed by the UI
/// data builder.
#[derive(Default)]
#[allow(dead_code)]
pub struct PowerSnapshot {
    /// Battery state of charge (0-100%) from the fuel gauge.
    pub battery_percent: Option<u8>,
    /// Battery terminal voltage in millivolts.
    pub battery_voltage_mv: Option<u16>,
}

pub struct PowerControls<'d> {
    sys_out: Output<'d>,
    motor: Output<'d>,
}

impl<'d> PowerControls<'d> {
    /// Initialize the power subsystem. Must be the first peripheral
    /// brought up at boot:
    ///
    /// 1. Latches SYS_OUT rail on (GPIO10 LOW)
    /// 2. Initializes the AXP2101 PMU and enables all power rails
    ///
    /// Returns `(PowerControls, Pmu)` on success. The caller wraps
    /// the `Pmu` in a `PowerTaskState` for the polling task.
    pub fn init(
        sys_out_pin: impl Into<Output<'d>>,
        motor_pin: impl Into<Output<'d>>,
        i2c: &mut impl I2c,
    ) -> Result<(Self, Pmu), ()> {
        let sys_out = sys_out_pin.into();
        let motor = motor_pin.into();

        let pmu = Pmu::new(PmuConfig::default());
        log::info!("PMU: initializing AXP2101...");
        match pmu.init(i2c) {
            Ok(raw_id) => {
                let version = (raw_id >> 4) & 0x03;
                log::info!(
                    "PMU: AXP2101 rev {} (0x{:02X}) - all rails enabled",
                    version, raw_id,
                );
            }
            Err(_) => {
                log::error!("PMU: initialization failed");
                return Err(());
            }
        }

        Ok((Self { sys_out, motor }, pmu))
    }

    /// Release the SYS_OUT latch - powers down the board.
    pub fn shutdown(&mut self) {
        log::info!("PWR: releasing SYS_OUT latch - powering off");
        self.sys_out.set_high();
    }

    /// Drive the haptic motor high (start buzz).
    pub fn buzz(&mut self) {
        self.motor.set_high();
    }

    /// Drive the haptic motor low (stop buzz).
    pub fn buzz_stop(&mut self) {
        self.motor.set_low();
    }
}
