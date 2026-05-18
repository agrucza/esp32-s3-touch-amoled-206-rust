//! The board seam.
//!
//! The small set of genuinely per-board operations the shared
//! `SystemManager` invokes. Each bin crate provides exactly one
//! `Board` impl; pins, peripheral construction and the partition
//! geometry never cross this trait - they're constructed in the bin
//! and passed to `SystemManager::new`. The manager is written once,
//! generic over `B: Board`, and is otherwise board-blind.
//!
//! Kept deliberately tiny (verified against the manager's actual
//! board-specific call sites): haptic motor, power-off, and the
//! wake-source arming done immediately before light sleep. The
//! manager owns everything else - the async FT3168 monitor write,
//! `rtc.sleep()`, the render/event loop - because none of it is
//! board-specific.

/// Per-board operations the shared manager calls.
pub trait Board {
    /// Start the haptic motor. Boards without one: no-op.
    fn buzz(&mut self);

    /// Stop the haptic motor. Boards without one: no-op.
    fn buzz_stop(&mut self);

    /// Power the board off. Either releases a soft-power latch or
    /// hands off to a PMU that manages long-press shutdown itself.
    fn shutdown(&mut self);

    /// Arm this board's hardware wake sources, synchronously, right
    /// before the manager enters light sleep. The manager owns *when*
    /// to sleep, the async FT3168 -> Monitor write, and the actual
    /// `rtc.sleep()` call (all board-agnostic); the board only
    /// declares *what wakes it* - which GPIO wake bits / internal
    /// timer to enable. Sync on purpose: no async-fn-in-trait across
    /// the two toolchains.
    fn arm_wake_sources(&mut self);
}
