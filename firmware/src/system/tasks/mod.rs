//! Per-device task state structs.
//!
//! Each submodule here owns the state for a single hardware device
//! and runs in its own `#[embassy_executor::task]`, spawned from
//! `main` after [`crate::system::manager::SystemManager::init`]
//! hands back a [`TaskBundle`]. Tasks communicate with the main
//! loop via the `EVENTS` channel in [`crate::system::bus`].
//!
//! [`TaskBundle`]: crate::system::manager::TaskBundle

pub mod boot_button;
pub mod imu;
pub mod power;
pub mod rtc;
pub mod touch;
