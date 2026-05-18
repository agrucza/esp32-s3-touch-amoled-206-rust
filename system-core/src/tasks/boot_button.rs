//! BOOT button (GPIO0) task state.
//!
//! Edge-detection on the BOOT button pin with software debouncing.
//! The button is active low (pulled up via 10K). Raw mechanical
//! contact produces multiple falling edges per physical press as
//! the contacts bounce, which without debouncing makes a single
//! press look like N rapid `BootButtonPressed` events - enough to
//! make the "BOOT while awake → sleep" / "BOOT while sleeping →
//! wake" handlers oscillate on a single press.
//!
//! Debounce strategy: on the first falling edge, emit one event.
//! Then wait for the pin to settle high again (release), and then
//! wait a short guard window before re-arming the falling-edge
//! detector. Bounces during the guard window are invisible to the
//! event channel.

use app_core::events::SystemEvent;
use crate::bus::EVENTS;
use embassy_time::{Duration, Timer};
use esp_hal::gpio::Input;

/// How long to wait after the button is seen high again (release)
/// before re-arming the falling-edge detector. Covers the trailing
/// contact bounce on release. 25 ms is comfortably above typical
/// tactile-switch bounce (~5 ms) without being noticeable to the
/// user even on fast repeated presses.
const RELEASE_GUARD_MS: u64 = 25;

/// BOOT button task: wait on falling edge, emit one event, wait
/// for release + settle, repeat.
#[embassy_executor::task]
pub async fn boot_button_task(mut state: BootButtonTaskState<'static>) {
    loop {
        state.wait_for_press().await;
        EVENTS.send(SystemEvent::BootButtonPressed).await;

        // Wait for the button to be released before looking for the
        // next press. Without this, contact bounce on the press edge
        // could immediately trigger another `wait_for_falling_edge`
        // and fire a phantom second event from the same physical
        // tap. `wait_for_rising_edge` returns as soon as the pin
        // reads high - bounces may still be rattling at that moment,
        // so we add a small guard delay afterwards to let them die
        // down before re-arming.
        state.wait_for_release().await;
        Timer::after(Duration::from_millis(RELEASE_GUARD_MS)).await;
    }
}

pub struct BootButtonTaskState<'d> {
    pin: Input<'d>,
    #[allow(dead_code)]
    prev_low: bool,
}

impl<'d> BootButtonTaskState<'d> {
    pub fn new(pin: Input<'d>) -> Self {
        Self { pin, prev_low: false }
    }

    /// Synchronous edge-detection poll. Intended for tick-time use
    /// until Phase 4 moves to a task-based wait.
    #[allow(dead_code)]
    pub fn poll(&mut self, events: &mut heapless::Vec<SystemEvent, 8>) {
        let now = self.pin.is_low();
        if now && !self.prev_low {
            let _ = events.push(SystemEvent::BootButtonPressed);
        }
        self.prev_low = now;
    }

    /// Async wait for the next falling edge (button press, active
    /// low). Paired with [`wait_for_release`] in the debounce loop.
    ///
    /// [`wait_for_release`]: BootButtonTaskState::wait_for_release
    pub async fn wait_for_press(&mut self) {
        self.pin.wait_for_falling_edge().await;
    }

    /// Async wait for the next rising edge (button release). Returns
    /// as soon as the pin reads high - bounces may still be in
    /// progress at that moment, so the caller should add a short
    /// settle delay before re-arming press detection.
    pub async fn wait_for_release(&mut self) {
        self.pin.wait_for_rising_edge().await;
    }
}
