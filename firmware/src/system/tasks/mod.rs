//! Per-device task state structs.
//!
//! Each submodule here owns the state for a single hardware device
//! that will eventually run in its own embassy task. For now these
//! are still polled synchronously from the main loop, but they're
//! structured so Phase 4 can `#[embassy_executor::task]`-ify them
//! with minimal churn.

pub mod boot_button;
pub mod imu;
pub mod power;
pub mod rtc;
pub mod touch;
