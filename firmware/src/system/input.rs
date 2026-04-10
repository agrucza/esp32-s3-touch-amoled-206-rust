use crate::events::{SwipeDir, SwipeRegion, SystemEvent};
use crate::ui::theme::{CONTENT_BOTTOM, CONTENT_TOP};
use drivers::touch::{FT3168, TouchEvent};
use embedded_hal::i2c::I2c;
use embassy_time::{Duration, Timer};
use esp_hal::gpio::{Input, Output};

/// Minimum travel distance on the dominant axis to count as a swipe (pixels).
const SWIPE_THRESHOLD: i32 = 60;

pub struct InputSystem<'d> {
    btn_boot: Input<'d>,
    btn_boot_prev: bool,
    touch: FT3168<Output<'d>>,
    touch_int: Input<'d>,
    /// First contact position of the current touch gesture (None while idle).
    touch_start: Option<(u16, u16)>,
    /// Last seen touch position, used to compute delta on release.
    touch_last: Option<(u16, u16)>,
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
            touch_start: None,
            touch_last: None,
        }
    }

    /// Classify a touch release into an optional swipe event.
    fn detect_swipe(start: (u16, u16), end: (u16, u16)) -> Option<SystemEvent> {
        let dx = end.0 as i32 - start.0 as i32;
        let dy = end.1 as i32 - start.1 as i32;
        let adx = dx.abs();
        let ady = dy.abs();

        // Pick the dominant axis and require it to exceed the threshold.
        let dir = if adx > ady {
            if adx < SWIPE_THRESHOLD { return None; }
            if dx > 0 { SwipeDir::Right } else { SwipeDir::Left }
        } else {
            if ady < SWIPE_THRESHOLD { return None; }
            if dy > 0 { SwipeDir::Down } else { SwipeDir::Up }
        };

        // Region is determined by where the gesture started.
        let region = if (start.1 as i32) < CONTENT_TOP {
            SwipeRegion::Header
        } else if (start.1 as i32) >= CONTENT_BOTTOM {
            SwipeRegion::Footer
        } else {
            SwipeRegion::Content
        };

        Some(SystemEvent::Swipe { dir, region })
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
                    if self.touch_start.is_none() {
                        self.touch_start = Some((x, y));
                    }
                    self.touch_last = Some((x, y));
                    let _ = events.push(SystemEvent::TouchPressed { x, y });
                }
                TouchEvent::Released => {
                    // Classify the gesture before clearing state.
                    if let (Some(start), Some(end)) = (self.touch_start, self.touch_last) {
                        if let Some(swipe) = Self::detect_swipe(start, end) {
                            let _ = events.push(swipe);
                        }
                    }
                    self.touch_start = None;
                    self.touch_last = None;
                    let _ = events.push(SystemEvent::TouchReleased);
                }
                TouchEvent::None => {}
            }
        }
    }
}
