//! Touch controller (FT3168) task state.
//!
//! Owns the FT3168 driver plus the INT line (GPIO38) and tracks
//! gesture state across polls so it can classify taps and swipes
//! on release. Emits `TouchPressed` / `TouchReleased` /
//! `Tap` / `Swipe` events.
//!
//! ## Phase 4 task loop sketch
//!
//! The outer loop is interrupt-driven: it sleeps on GPIO38's
//! falling edge, which the FT3168 asserts when it has new
//! sample data. Once a finger is down, the inner loop polls
//! the controller at ~120 Hz to capture drag samples, until
//! `is_touching()` reports a release. This works regardless
//! of whether the FT3168 pulses INT# per sample or holds it
//! low continuously during a press.
//!
//! ```ignore
//! #[embassy_executor::task]
//! async fn touch_task(bus: &'static SharedI2c, mut state: TouchTaskState<'static>) {
//!     loop {
//!         state.wait_for_int().await;             // GPIO38 falling edge
//!         loop {
//!             let mut i2c = bus.lock().await;
//!             let mut events = heapless::Vec::<SystemEvent, 4>::new();
//!             state.read_events(&mut *i2c, &mut events);
//!             drop(i2c);
//!             for ev in events { EVENTS.send(ev).await; }
//!             if !state.is_touching() { break; }   // back to INT wait
//!             Timer::after(Duration::from_millis(8)).await;  // ~120 Hz drag
//!         }
//!     }
//! }
//! ```

use crate::events::{SwipeDir, SwipeRegion, SystemEvent};
use crate::ui::theme::{EDGE_GESTURE_ZONE, SCREEN_H, SCREEN_W};
use drivers::touch::{FT3168, TouchEvent};
use embassy_time::{Duration, Timer};
use embedded_hal::i2c::I2c as I2cTrait;
use esp_hal::gpio::{Input, Output};

/// Minimum travel distance on the dominant axis to count as a swipe (pixels).
const SWIPE_THRESHOLD: i32 = 60;

/// Current touch point. Both fields are `None` when no finger is
/// down. Updated incrementally from `TouchPressed` / `TouchReleased`
/// events by the main event handler - no I2C reads required.
#[derive(Debug, Clone, Copy, Default)]
pub struct TouchData {
    pub x: Option<u16>,
    pub y: Option<u16>,
}

pub struct TouchTaskState<'d> {
    touch: FT3168<Output<'d>>,
    touch_int: Input<'d>,
    /// First contact position of the current touch gesture (None while idle).
    touch_start: Option<(u16, u16)>,
    /// Last seen touch position, used to compute delta on release.
    touch_last: Option<(u16, u16)>,
}

impl<'d> TouchTaskState<'d> {
    /// Perform the touch reset sequence and verify the chip
    /// responds. Returns a ready-to-use TouchTaskState.
    pub async fn init(
        reset_pin: Output<'d>,
        int_pin: Input<'d>,
        i2c: &mut impl I2cTrait,
    ) -> Self {
        let mut touch = FT3168::new(reset_pin);

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
            touch,
            touch_int: int_pin,
            touch_start: None,
            touch_last: None,
        }
    }

    /// Classify a touch release into a swipe or tap event.
    fn classify_gesture(start: (u16, u16), end: (u16, u16)) -> SystemEvent {
        let dx = end.0 as i32 - start.0 as i32;
        let dy = end.1 as i32 - start.1 as i32;
        let adx = dx.abs();
        let ady = dy.abs();

        // Pick the dominant axis and require it to exceed the threshold.
        let dir = if adx > ady {
            if adx < SWIPE_THRESHOLD {
                return SystemEvent::Tap { x: start.0, y: start.1 };
            }
            if dx > 0 { SwipeDir::Right } else { SwipeDir::Left }
        } else {
            if ady < SWIPE_THRESHOLD {
                return SystemEvent::Tap { x: start.0, y: start.1 };
            }
            if dy > 0 { SwipeDir::Down } else { SwipeDir::Up }
        };

        // Region is where the gesture started. Top/Bottom take
        // precedence over Left/Right when the start point falls
        // into a corner zone.
        let start_x = start.0 as i32;
        let start_y = start.1 as i32;
        let screen_w = SCREEN_W as i32;
        let screen_h = SCREEN_H as i32;
        let region = if start_y < EDGE_GESTURE_ZONE {
            SwipeRegion::Top
        } else if start_y >= screen_h - EDGE_GESTURE_ZONE {
            SwipeRegion::Bottom
        } else if start_x < EDGE_GESTURE_ZONE {
            SwipeRegion::Left
        } else if start_x >= screen_w - EDGE_GESTURE_ZONE {
            SwipeRegion::Right
        } else {
            SwipeRegion::Content
        };

        SystemEvent::Swipe { dir, region }
    }

    /// Async wait for the touch INT line to go low. The FT3168
    /// asserts this when it has new sample data. Call
    /// `read_events` after this returns to extract what happened.
    pub async fn wait_for_int(&mut self) {
        self.touch_int.wait_for_falling_edge().await;
    }

    /// Read the FT3168 over I2C and emit gesture events. Intended
    /// to be called after `wait_for_int` fires - it's a pure
    /// "interpret the controller's current state" operation, not
    /// a periodic poll.
    pub fn read_events(
        &mut self,
        i2c: &mut impl I2cTrait,
        events: &mut heapless::Vec<SystemEvent, 8>,
    ) {
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
                    let _ = events.push(SystemEvent::TouchReleased);
                    if let (Some(start), Some(end)) = (self.touch_start, self.touch_last) {
                        let _ = events.push(Self::classify_gesture(start, end));
                    }
                    self.touch_start = None;
                    self.touch_last = None;
                }
                TouchEvent::None => {}
            }
        }
    }

    /// Current touch point as a `TouchData` snapshot. This is
    /// purely local state (no I2C read) since the task tracks
    /// the current press position in `touch_last`.
    pub fn snapshot(&self) -> TouchData {
        match self.touch_last {
            Some((x, y)) => TouchData { x: Some(x), y: Some(y) },
            None => TouchData::default(),
        }
    }

    /// Returns `true` while a finger is still pressed on the
    /// screen (after the first `TouchPressed` and before the
    /// `TouchReleased`). Used by the task loop to decide whether
    /// to keep reading the controller at drag-sample cadence or
    /// go back to sleeping on INT#.
    pub fn is_touching(&self) -> bool {
        self.touch_last.is_some()
    }
}
