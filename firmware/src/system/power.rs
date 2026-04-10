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

    pub fn buzz(&mut self) {
        self.motor.set_high();
    }

    pub fn buzz_stop(&mut self) {
        self.motor.set_low();
    }
}
