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
//! board-specific call sites): haptic motor, power-off, wake-source
//! arming, CPU-frequency scaling, and light-sleep config tuning - the
//! last two because the clock/sleep registers differ per chip family
//! (e.g. the s3 `SYSTEM` block vs the c6 `PCR` block). The manager
//! owns everything else - the async FT3168 monitor write,
//! `rtc.sleep()`, the render/event loop, and the *policy* of when to
//! scale/sleep - because none of that is board-specific.

/// CPU clock levels the manager scales between. A board-agnostic
/// concept; the actual register sequence to reach each level is
/// chip-specific and lives behind [`Board::set_cpu_freq`]. The
/// manager only uses `Mhz80` (idle/sleep baseline) and `Mhz160`
/// (render boost) at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuFreq {
    /// 80 MHz - baseline for idle / pre-sleep.
    Mhz80,
    /// 160 MHz - render boost. Highest level on chips capped here.
    Mhz160,
    /// 240 MHz - not reached at runtime by the manager, and not
    /// achievable on every chip (e.g. the c6 tops out at 160). A
    /// board may clamp this to its maximum.
    #[allow(dead_code)]
    Mhz240,
}

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

    /// Switch the CPU clock to `freq`. The chip's clock registers
    /// differ per family, so the sequence lives here. A board may
    /// clamp a level it can't reach (e.g. `Mhz240` -> its max). The
    /// manager owns the *policy* (boost for render, drop for idle/
    /// sleep); the board owns the *poke*.
    fn set_cpu_freq(&mut self, freq: CpuFreq);

    /// Apply this board's chip-specific reliability tuning to the
    /// `RtcSleepConfig` the manager is about to sleep with. The
    /// available knobs differ per chip family (the s3 fpu/reject
    /// fields don't exist on the c6 `RtcSleepConfig`), so each board
    /// tunes what its silicon supports; the manager owns the default
    /// config, the wake sources, and the `rtc.sleep()` call.
    fn tune_sleep_config(
        &self,
        cfg: &mut esp_hal::rtc_cntl::sleep::RtcSleepConfig,
    );
}
