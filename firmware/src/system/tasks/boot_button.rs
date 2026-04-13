//! BOOT button (GPIO0) task state.
//!
//! Simple edge-detection on the BOOT button pin. Active low
//! (pulled up via 10K). Emits `SystemEvent::BootButtonPressed`
//! on a falling edge.
//!
//! ## Phase 4 task loop sketch
//!
//! Nothing to it - the pin is a direct ESP32 GPIO with an
//! async wait, so the whole task is one await + one send.
//!
//! ```ignore
//! #[embassy_executor::task]
//! async fn boot_button_task(mut state: BootButtonTaskState<'static>) {
//!     loop {
//!         state.wait_for_press().await;
//!         EVENTS.send(SystemEvent::BootButtonPressed).await;
//!     }
//! }
//! ```

use crate::events::SystemEvent;
use crate::system::bus::EVENTS;
use esp_hal::gpio::Input;

/// BOOT button task: wait on falling edge, push event, repeat.
#[embassy_executor::task]
pub async fn boot_button_task(mut state: BootButtonTaskState<'static>) {
    loop {
        state.wait_for_press().await;
        EVENTS.send(SystemEvent::BootButtonPressed).await;
    }
}

pub struct BootButtonTaskState<'d> {
    pin: Input<'d>,
    prev_low: bool,
}

impl<'d> BootButtonTaskState<'d> {
    pub fn new(pin: Input<'d>) -> Self {
        Self { pin, prev_low: false }
    }

    /// Synchronous edge-detection poll. Intended for tick-time use
    /// until Phase 4 moves to a task-based wait.
    pub fn poll(&mut self, events: &mut heapless::Vec<SystemEvent, 8>) {
        let now = self.pin.is_low();
        if now && !self.prev_low {
            let _ = events.push(SystemEvent::BootButtonPressed);
        }
        self.prev_low = now;
    }

    /// Async wait for the next falling edge. Used by the main loop
    /// today as part of `wait_for_user_input`; will become the wait
    /// inside the BOOT button task in Phase 4.
    pub async fn wait_for_press(&mut self) {
        self.pin.wait_for_falling_edge().await;
    }
}
