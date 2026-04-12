use crate::events::SystemEvent;
use drivers::pmu::{Pmu, Config as PmuConfig, InterruptSource};
use embedded_hal::i2c::I2c;
use esp_hal::gpio::Output;

pub struct PowerSystem<'d> {
    pmu: Pmu,
    sys_out: Output<'d>,
    motor: Output<'d>,
    last_battery: u8,
}

impl<'d> PowerSystem<'d> {
    /// Initialize the power system. Must be called before any other subsystem.
    ///
    /// - Latches SYS_OUT rail on (GPIO10 LOW)
    /// - Initializes AXP2101 PMU and enables all power rails
    pub fn init(
        sys_out_pin: impl Into<Output<'d>>,
        motor_pin: impl Into<Output<'d>>,
        i2c: &mut impl I2c,
    ) -> Result<Self, ()> {
        let sys_out = sys_out_pin.into();
        let motor = motor_pin.into();

        let pmu = Pmu::new(PmuConfig::default());
        log::info!("PMU: initializing AXP2101...");
        match pmu.init(i2c) {
            Ok(raw_id) => {
                let version = (raw_id >> 4) & 0x03;
                log::info!("PMU: AXP2101 rev {} (0x{:02X}) - all rails enabled", version, raw_id);
            }
            Err(_) => {
                log::error!("PMU: initialization failed");
                return Err(());
            }
        }

        Ok(Self {
            pmu,
            sys_out,
            motor,
            last_battery: 0xFF,
        })
    }

    /// Poll PMU interrupts and battery level, push events for any changes.
    pub fn poll(&mut self, i2c: &mut impl I2c, events: &mut heapless::Vec<SystemEvent, 8>) {
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

    pub fn buzz(&mut self) {
        self.motor.set_high();
    }

    pub fn buzz_stop(&mut self) {
        self.motor.set_low();
    }
}
