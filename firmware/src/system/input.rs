use crate::events::SystemEvent;
use drivers::touch::{FT3168, TouchEvent};
use embedded_hal::i2c::I2c;
use embassy_time::{Duration, Timer};
use esp_hal::gpio::{Input, Output};

pub struct InputSystem<'d> {
    btn_boot: Input<'d>,
    btn_boot_prev: bool,
    touch: FT3168<Output<'d>>,
    touch_int: Input<'d>,
}

impl<'d> InputSystem<'d> {
    /// Initialize buttons and touch controller.
    pub async fn init(
        boot_pin: impl Into<Input<'d>>,
        touch_rst_pin: impl Into<Output<'d>>,
        touch_int_pin: impl Into<Input<'d>>,
        i2c: &mut impl I2c,
    ) -> Self {
        let btn_boot = boot_pin.into();
        let touch_int = touch_int_pin.into();
        let mut touch = FT3168::new(touch_rst_pin.into());

        // Touch reset sequence
        touch.reset_low();
        Timer::after(Duration::from_millis(10)).await;
        touch.reset_high();
        Timer::after(Duration::from_millis(50)).await;

        log::info!("Touch: initializing FT3168...");
        match touch.read_ids(i2c) {
            Ok((chip_id, fw_ver)) => {
                log::info!("Touch: chip ID=0x{:02X}, FW version=0x{:02X}", chip_id, fw_ver);
            }
            Err(_) => log::error!("Touch: device not found at I2C address 0x{:02X}", drivers::touch::ADDR),
        }

        Self {
            btn_boot,
            btn_boot_prev: false,
            touch,
            touch_int,
        }
    }

    /// Poll all input sources and push events into the buffer.
    pub fn poll(&mut self, i2c: &mut impl I2c, events: &mut heapless::Vec<SystemEvent, 8>) {
        // BOOT button - falling edge detection
        let now = self.btn_boot.is_low();
        if now && !self.btn_boot_prev {
            let _ = events.push(SystemEvent::BootButtonPressed);
        }
        self.btn_boot_prev = now;

        // Touch controller
        if self.touch_int.is_low() || self.touch.is_pressed() {
            match self.touch.read(i2c) {
                TouchEvent::Pressed { x, y } => {
                    let _ = events.push(SystemEvent::TouchPressed { x, y });
                }
                TouchEvent::Released => {
                    let _ = events.push(SystemEvent::TouchReleased);
                }
                TouchEvent::None => {}
            }
        }
    }
}
