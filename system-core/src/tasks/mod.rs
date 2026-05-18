//! Per-device task state structs.
//!
//! Each submodule here owns the state for a single hardware device
//! and runs in its own `#[embassy_executor::task]`, spawned from the
//! bin's `main` after `SystemManager::new` hands back a task bundle.
//! Tasks communicate with the main loop via the `EVENTS` channel in
//! [`crate::bus`].

pub mod boot_button;
pub mod imu;
pub mod power;
pub mod rtc;
pub mod touch;
