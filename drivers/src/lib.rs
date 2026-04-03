//! drivers - HAL-agnostic peripheral drivers.
//!
//! Each driver is written against `embedded-hal` / `embedded-hal-async`
//! traits, not against `esp-hal` types directly. This means:
//!
//!  - Drivers are portable across any HAL that implements those traits.
//!  - Drivers can be unit-tested on the host with `embedded-hal-mock`.
//!
//! Modules are added milestone by milestone:
//!   Milestone 1: firmware (BASIC BUILD)
//!   Milestone 2: display  (RM67162 QSPI AMOLED)
//!   Milestone 3: touch    (FT3168 I2C)
//!   Milestone 4: pmu      (AXP2101 I2C)

#![no_std]

#[cfg(feature = "display")]
pub mod display;

#[cfg(feature = "touch")]
pub mod touch;

#[cfg(feature = "pmu")]
pub mod pmu;

#[cfg(feature = "rtc")]
pub mod rtc;

pub use embedded_hal;

#[cfg(feature = "defmt")]
pub use defmt;

#[cfg(feature = "pmu")]
pub use pmu::*;
