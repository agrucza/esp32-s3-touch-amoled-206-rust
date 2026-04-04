use crate::events::SystemEvent;
use drivers::pmu::{Pmu, Config as PmuConfig, InterruptSource};
use embedded_hal::i2c::I2c;
use esp_hal::gpio::Output;

pub struct PowerSystem<'d> {
    pmu: Pmu,
    _sys_out: Output<'d>,
    motor: Output<'d>,
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
            _sys_out: sys_out,
            motor,
        })
    }

    /// Poll PMU interrupt registers and return any power button events.
    pub fn poll(&self, i2c: &mut impl I2c, events: &mut heapless::Vec<SystemEvent, 8>) {
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
    }

    pub fn buzz(&mut self) {
        self.motor.set_high();
    }

    pub fn buzz_stop(&mut self) {
        self.motor.set_low();
    }
}
