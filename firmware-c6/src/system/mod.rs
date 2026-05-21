//! The board-agnostic system layer lives in `system-core`.
//!
//! All that's left here is `power` - the C6
//! `system_core::board::Board` impl + AXP2101 sanity check.
//! Genuinely board-specific, so it stays bin-side. Everything else
//! (manager, tasks, bus, display, storage, audio) lives in
//! `system-core` and is used from there directly by `main.rs`.
//! Mirrors `firmware-s3/src/system/`.

pub mod power;
