#![no_std]

//! Shared hardware-init helpers for the ESP32-{S3, C6}-Touch-AMOLED-2.06 boards.
//!
//! See the crate-level comment in `Cargo.toml` for the design intent: this
//! crate hosts esp-hal-coupled init code that both `firmware-s3` and
//! `firmware-c6` consume. Board-specific pin maps and main-loop wiring stay
//! in the bin crates.

pub mod display;
