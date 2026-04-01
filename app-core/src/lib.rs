//! app-core — hardware-agnostic application logic.
//!
//! This crate is `no_std` when compiled for the embedded target, but compiles
//! with `std` on the host so that `cargo test -p app-core` works without a
//! physical device or cross-compiler.
//!
//! Rules:
//!  - No `esp-hal`, no GPIO, no SPI, no I2C.
//!  - Only pure logic: state machines, event types, UI model, data transforms.
//!  - Everything here is covered by host-side unit tests.

#![cfg_attr(not(test), no_std)]

/// Top-level application events produced by hardware tasks and consumed by the
/// app state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum AppEvent {
    /// A touch point was registered at (x, y) in display coordinates.
    Touch { x: u16, y: u16 },
    /// The touch surface was released.
    TouchRelease,
    /// A timer tick at a regular interval (ms since boot).
    Tick(u64),
}

/// Top-level application state — extend as the UI grows.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AppState {
    pub tick_count: u64,
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply an event and return the updated state.
    pub fn update(&mut self, event: &AppEvent) {
        match event {
            AppEvent::Tick(_) => self.tick_count += 1,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_increments_count() {
        let mut state = AppState::new();
        state.update(&AppEvent::Tick(1000));
        state.update(&AppEvent::Tick(2000));
        assert_eq!(state.tick_count, 2);
    }

    #[test]
    fn touch_does_not_increment_tick() {
        let mut state = AppState::new();
        state.update(&AppEvent::Touch { x: 10, y: 20 });
        assert_eq!(state.tick_count, 0);
    }
}
