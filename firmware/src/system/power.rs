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
#[derive(Default)]
pub struct PowerSnapshot {
    /// Battery state of charge (0-100%) from the fuel gauge.
    pub battery_percent: Option<u8>,
    /// Battery terminal voltage in millivolts.
    pub battery_voltage_mv: Option<u16>,
}

/// Snapshot of power-related readings at one point in time.
/// Produced by `PowerTaskState::snapshot`; consumed by the UI
/// data builder.
#[derive(Default)]
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

    /// Read battery percentage (0-100%).
    pub fn battery_percent(&self, i2c: &mut impl I2c) -> Option<u8> {
        self.pmu.battery_percent(i2c).ok()
    }

    /// Read battery voltage in millivolts.
    pub fn battery_voltage_mv(&self, i2c: &mut impl I2c) -> Option<u16> {
        self.pmu.battery_voltage_mv(i2c).ok()
    }

    /// Collect all power-related readings into a single snapshot.
    pub fn snapshot(&self, i2c: &mut impl I2c) -> PowerSnapshot {
        PowerSnapshot {
            battery_percent: self.pmu.battery_percent(i2c).ok(),
            battery_voltage_mv: self.pmu.battery_voltage_mv(i2c).ok(),
        }
    }

    /// Log a diagnostic dump of all readable PMU state.
    ///
    /// Call once after init to verify register reads match the
    /// physical hardware state (USB plugged in, battery level, etc.).
    pub fn dump_status(&self, i2c: &mut impl I2c) {
        // Status registers
        if let Ok(s1) = self.pmu.read_status1(i2c) {
            log::info!(
                "PMU status1: vbus_good={} batfet={} bat_present={} bat_active={} thermal={} ilim={}",
                s1.vbus_good, s1.batfet_active, s1.battery_present,
                s1.battery_active, s1.thermal_active, s1.current_limit_active,
            );
        }
        if let Ok(s2) = self.pmu.read_status2(i2c) {
            log::info!(
                "PMU status2: direction={:?} phase={:?} system_on={} vindpm={}",
                s2.current_direction, s2.charger_phase, s2.system_on, s2.vindpm_active,
            );
        }

        // Power-on/off sources
        if let Ok(on) = self.pmu.power_on_status(i2c) {
            log::info!(
                "PMU poweron: button={} vbus={} bat_insert={} bat_charged={} irq={} en={}",
                on.button, on.vbus, on.battery_insert, on.battery_charged, on.irq_pin, on.en_mode,
            );
        }
        if let Ok(off) = self.pmu.power_off_status(i2c) {
            log::info!(
                "PMU pwroff: button={} sw={} die_ot={} dcdc_ov={} dcdc_uv={} vbus_ov={} vsys_uv={} en={}",
                off.button_long_press, off.software, off.die_overtemp,
                off.dcdc_overvolt, off.dcdc_undervolt, off.vbus_overvolt,
                off.vsys_undervolt, off.en_mode,
            );
        }

        // Battery and ADC
        if let Ok(mv) = self.pmu.battery_voltage_mv(i2c) {
            log::info!("PMU battery: {} mV", mv);
        }
        if let Ok(pct) = self.pmu.battery_percent(i2c) {
            log::info!("PMU battery: {}%", pct);
        }
        if let Ok(mv) = self.pmu.vbus_voltage_mv(i2c) {
            log::info!("PMU vbus: {} mV", mv);
        }
        if let Ok(mv) = self.pmu.system_voltage_mv(i2c) {
            log::info!("PMU vsys: {} mV", mv);
        }
        if let Ok(raw) = self.pmu.die_temperature_raw(i2c) {
            log::info!("PMU die temp: raw={}", raw);
        }

        // Charger config readback
        if let Ok(cc) = self.pmu.charge_current(i2c) {
            log::info!("PMU charge current: {} mA", cc.as_ma());
        }
        if let Ok(cv) = self.pmu.charge_voltage(i2c) {
            log::info!("PMU charge voltage: {:?}", cv);
        }
        if let Ok(ilim) = self.pmu.input_current_limit(i2c) {
            log::info!("PMU input current limit: {:?}", ilim);
        }
        if let Ok(vindpm) = self.pmu.input_voltage_limit(i2c) {
            log::info!("PMU input voltage limit: {} mV", vindpm);
        }

        // Power key timing
        if let Ok(pk) = self.pmu.power_key_config(i2c) {
            log::info!(
                "PMU power key: irq={:?} off={:?} on={:?}",
                pk.irq_time, pk.off_time, pk.on_time,
            );
        }
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
