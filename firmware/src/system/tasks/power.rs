//! Power / PMU (AXP2101) task state.
//!
//! Owns the AXP2101 I2C driver. On the ESP32-S3-Touch-AMOLED-2.06
//! the AXP2101 IRQ output is tied to the `EXIO5` net which is
//! used as the RTC-VCC power rail - it's not routed to any usable
//! signal, so there's no async wake source for PMU events. The
//! task polls the PMU every ~500 ms, reads the full state via
//! `snapshot()`, diffs against the previous snapshot, and emits
//! specific change events: power button presses, battery percent
//! changes, VBUS plug/unplug, charger phase transitions, etc.
//!
//! This is the only task in the Phase 4 model that uses a timer
//! instead of an async interrupt wait. Polling is the only option
//! because the AXP2101 IRQ output isn't readable on this board.
//!
//! Note: motor (haptic) and SYS_OUT (power latch) GPIOs live in
//! `system::power::PowerControls` - those stay with the main task
//! because `handle_events` needs to buzz the motor during BOOT
//! press handling and release SYS_OUT on shutdown.
//!
//! ## Phase 4 task loop sketch
//!
//! ```ignore
//! #[embassy_executor::task]
//! async fn power_task(bus: &'static SharedI2c, mut state: PowerTaskState) {
//!     let mut prev = PowerData::default();
//!     loop {
//!         Timer::after(Duration::from_millis(POLL_INTERVAL_MS)).await;
//!
//!         // Drain any pending button-press IRQs and read the
//!         // full PMU state in one bus lock window.
//!         let mut i2c = bus.lock().await;
//!         let mut events = heapless::Vec::<SystemEvent, 8>::new();
//!         state.poll(&mut *i2c, &mut events);     // button IRQs + battery%
//!         let fresh = state.snapshot(&mut *i2c);   // full PowerData
//!         drop(i2c);
//!
//!         // Forward button events first.
//!         for event in events { EVENTS.send(event).await; }
//!
//!         // Diff fresh vs prev and emit transition events.
//!         if fresh.vbus_good && !prev.vbus_good {
//!             EVENTS.send(SystemEvent::VbusInserted).await;
//!         } else if !fresh.vbus_good && prev.vbus_good {
//!             EVENTS.send(SystemEvent::VbusRemoved).await;
//!         }
//!         if fresh.charger_phase != prev.charger_phase {
//!             EVENTS.send(SystemEvent::ChargerPhaseChanged {
//!                 phase: fresh.charger_phase,
//!             }).await;
//!         }
//!         if fresh.current_direction != prev.current_direction {
//!             EVENTS.send(SystemEvent::CurrentDirectionChanged {
//!                 direction: fresh.current_direction,
//!             }).await;
//!         }
//!         prev = fresh;
//!     }
//! }
//! ```

use crate::events::SystemEvent;
use crate::system::bus::{EVENTS, SharedI2c};
use drivers::pmu::{
    ChargeVoltage, ChargerPhase, CurrentDirection, InputCurrentLimit, InterruptSource, Pmu,
};
use embassy_time::{Duration, Timer};
use embedded_hal::i2c::I2c as I2cTrait;

/// Power task: poll the AXP2101 every [`POLL_INTERVAL_MS`], diff
/// against the previous snapshot, and emit specific change events
/// (button press, VBUS plug/unplug, charger phase, battery %).
#[embassy_executor::task]
pub async fn power_task(bus: &'static SharedI2c, mut state: PowerTaskState) {
    let mut prev = PowerData::default();
    let mut first = true;
    loop {
        Timer::after(Duration::from_millis(POLL_INTERVAL_MS)).await;

        let (fresh, events) = {
            let mut i2c = bus.lock().await;
            let mut events: heapless::Vec<SystemEvent, 8> = heapless::Vec::new();
            state.poll(&mut *i2c, &mut events);
            let fresh = state.snapshot(&mut *i2c);
            (fresh, events)
        };

        // Forward button / battery% events surfaced by poll().
        for event in events {
            EVENTS.send(event).await;
        }

        // Push the full snapshot so the main loop's cache can
        // keep fields like VBUS voltage, system voltage, charger
        // config etc. current without ever touching the bus.
        EVENTS.send(SystemEvent::PowerUpdated { data: fresh }).await;

        // Diff the full snapshot against the last one and emit
        // transition events. Skip the first iteration so we don't
        // spam phantom transitions from the default-initialised
        // `prev` against a real reading.
        if !first {
            if fresh.vbus_good && !prev.vbus_good {
                EVENTS.send(SystemEvent::VbusInserted).await;
            } else if !fresh.vbus_good && prev.vbus_good {
                EVENTS.send(SystemEvent::VbusRemoved).await;
            }
            if fresh.charger_phase != prev.charger_phase {
                EVENTS.send(SystemEvent::ChargerPhaseChanged {
                    phase: fresh.charger_phase,
                }).await;
            }
            if fresh.current_direction != prev.current_direction {
                EVENTS.send(SystemEvent::CurrentDirectionChanged {
                    direction: fresh.current_direction,
                }).await;
            }
        }
        prev = fresh;
        first = false;
    }
}

/// Polling interval for the PMU task in milliseconds.
pub const POLL_INTERVAL_MS: u64 = 500;

/// Complete power / battery / charger state read from the AXP2101.
///
/// All status-register bits are flattened into individual fields
/// so screens can read `data.power.vbus_good` directly without
/// going through a nested struct. Fields that come from an I2C
/// read that can fail are `Option<_>`; status flags default to
/// their inactive state when the read fails (screens treat that
/// as "nothing is happening").
#[derive(Debug, Clone, Copy, Default)]
pub struct PowerData {
    // --- Battery ---
    pub battery_percent: Option<u8>,
    pub battery_voltage_mv: Option<u16>,

    // --- Power path (from PMU status register 1) ---
    /// VBUS is present and above the VBUS good threshold.
    pub vbus_good: bool,
    /// BATFET is on (battery connected to the power path).
    pub batfet_active: bool,
    /// Battery is detected by the charger.
    pub battery_present: bool,
    /// Battery is in active (non-sleep) mode.
    pub battery_active: bool,
    /// Die is in thermal regulation (charging current reduced).
    pub thermal_active: bool,
    /// Input current limit regulation is active.
    pub current_limit_active: bool,

    // --- Charger state (from PMU status register 2) ---
    /// Battery current direction (standby / charging / discharging).
    pub current_direction: CurrentDirection,
    /// Charger phase (tri-charge / pre-charge / CC / CV / done / not charging).
    pub charger_phase: ChargerPhase,
    /// System is powered on (always true while we're running).
    pub system_on: bool,
    /// VINDPM regulation is active (input voltage at limit).
    pub vindpm_active: bool,

    // --- ADC readings ---
    pub vbus_voltage_mv: Option<u16>,
    pub system_voltage_mv: Option<u16>,
    pub die_temperature_raw: Option<u16>,

    // --- Charger config (typically static, read once to verify) ---
    pub charge_current_ma: Option<u16>,
    pub charge_voltage: Option<ChargeVoltage>,
    pub input_current_limit: Option<InputCurrentLimit>,
    pub input_voltage_limit_mv: Option<u16>,
}

pub struct PowerTaskState {
    pub pmu: Pmu,
    last_battery: u8,
}

impl PowerTaskState {
    /// Create the PMU handle around the driver. The rails should
    /// already be enabled by `PowerControls::init` before this is
    /// called - this struct only owns the polling/IRQ side.
    pub fn new(pmu: Pmu) -> Self {
        Self { pmu, last_battery: 0xFF }
    }

    /// Poll the PMU for interrupt sources (power button events)
    /// and battery percentage. Emits corresponding events.
    ///
    /// Called every ~500 ms during Phase 3 tick-time polling;
    /// becomes the body of the power task loop in Phase 4.
    pub fn poll(&mut self, i2c: &mut impl I2cTrait, events: &mut heapless::Vec<SystemEvent, 8>) {
        // Power button interrupts
        if let Ok(irq) = self.pmu.read_interrupts(i2c) {
            if !irq.is_empty() {
                if irq.is_active(InterruptSource::PowerOnShortPress) {
                    let _ = events.push(SystemEvent::PowerButtonShort);
                }
                if irq.is_active(InterruptSource::PowerOnLongPress) {
                    let _ = events.push(SystemEvent::PowerButtonLong);
                }
                let _ = self.pmu.clear_interrupts(i2c, &irq);
            }
        }

        // Battery percentage change
        if let Ok(pct) = self.pmu.battery_percent(i2c) {
            if pct != self.last_battery {
                self.last_battery = pct;
                let _ = events.push(SystemEvent::BatteryChanged { percent: pct });
            }
        }
    }

    /// Read battery percentage for the render snapshot.
    pub fn battery_percent(&self, i2c: &mut impl I2cTrait) -> Option<u8> {
        self.pmu.battery_percent(i2c).ok()
    }

    /// Read battery voltage for the render snapshot.
    pub fn battery_voltage_mv(&self, i2c: &mut impl I2cTrait) -> Option<u16> {
        self.pmu.battery_voltage_mv(i2c).ok()
    }

    /// Collect a full `PowerData` snapshot from the PMU. Reads ~13
    /// I2C transactions, so don't call this in a hot loop - it's
    /// intended for boot, wake-from-sleep, and the 500 ms poll
    /// cadence inside the power task.
    pub fn snapshot(&self, i2c: &mut impl I2cTrait) -> PowerData {
        let status1 = self.pmu.read_status1(i2c).unwrap_or_default();
        let status2 = self.pmu.read_status2(i2c).unwrap_or_default();
        PowerData {
            // Battery
            battery_percent: self.pmu.battery_percent(i2c).ok(),
            battery_voltage_mv: self.pmu.battery_voltage_mv(i2c).ok(),

            // Power path (status1)
            vbus_good: status1.vbus_good,
            batfet_active: status1.batfet_active,
            battery_present: status1.battery_present,
            battery_active: status1.battery_active,
            thermal_active: status1.thermal_active,
            current_limit_active: status1.current_limit_active,

            // Charger state (status2)
            current_direction: status2.current_direction,
            charger_phase: status2.charger_phase,
            system_on: status2.system_on,
            vindpm_active: status2.vindpm_active,

            // ADC readings
            vbus_voltage_mv: self.pmu.vbus_voltage_mv(i2c).ok(),
            system_voltage_mv: self.pmu.system_voltage_mv(i2c).ok(),
            die_temperature_raw: self.pmu.die_temperature_raw(i2c).ok(),

            // Charger config
            charge_current_ma: self.pmu.charge_current(i2c).ok().map(|cc| cc.as_ma()),
            charge_voltage: self.pmu.charge_voltage(i2c).ok().flatten(),
            input_current_limit: self.pmu.input_current_limit(i2c).ok(),
            input_voltage_limit_mv: self.pmu.input_voltage_limit(i2c).ok(),
        }
    }

    /// Log a diagnostic dump of all readable PMU state.
    ///
    /// Call once after init to verify register reads match the
    /// physical hardware state (USB plugged in, battery level, etc.).
    pub fn dump_status(&self, i2c: &mut impl I2cTrait) {
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
}
