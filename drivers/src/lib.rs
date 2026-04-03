//! drivers - HAL-agnostic peripheral drivers.
//!
//! Each driver is written against `embedded-hal` / `embedded-hal-async`
//! traits, not against `esp-hal` types directly. This means:
//!
//!  - Drivers are portable across any HAL that implements those traits.
//!  - Drivers can be unit-tested on the host with `embedded-hal-mock`.

#![no_std]

#[cfg(feature = "display")]
pub mod display;

#[cfg(feature = "touch")]
pub mod touch;

#[cfg(feature = "pmu")]
pub mod pmu;

#[cfg(feature = "imu")]
pub mod imu;

#[cfg(feature = "rtc")]
pub mod rtc;

pub use embedded_hal;

#[cfg(feature = "defmt")]
pub use defmt;

#[cfg(feature = "pmu")]
pub use pmu::*;
