//! The board-agnostic system layer lives in `system-core`.
//!
//! All that's left here is `power` - the S3
//! `system_core::board::Board` impl (GPIO10 latch + motor + the S3
//! light-sleep wake-pin arming). Genuinely board-specific, so it
//! stays bin-side. Everything else (`manager`, `tasks`, `bus`,
//! `display`, the storage stack, `audio`) lives in `system-core` and
//! is used from there directly by `main.rs`.

pub mod power;
