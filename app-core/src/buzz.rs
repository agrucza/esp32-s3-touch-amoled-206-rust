//! Buzz-pattern state machine.
//!
//! A buzz pattern is a repeating on/off cycle with independent
//! durations (e.g. 100 ms on, 400 ms off for a soft alarm). The
//! firmware's motor driver is a dumb on/off GPIO, so something
//! has to track the elapsed time since the last transition and
//! tell the driver when to flip.
//!
//! This module is pure logic: the pattern takes the current time
//! as input on each tick and returns what the caller should do to
//! the hardware. Callers own the actual GPIO / PMU-motor handle.

use embassy_time::Instant;

/// What the caller should do to the motor hardware right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuzzAction {
    /// Leave the motor in its current state.
    None,
    /// Drive the motor on (start of an "on" phase).
    TurnOn,
    /// Drive the motor off (start of an "off" phase).
    TurnOff,
}

/// Active buzz pattern state. The caller calls [`Self::tick`] on
/// every tick with the current time and drives the motor based
/// on the returned [`BuzzAction`].
#[derive(Debug, Clone, Copy)]
pub struct BuzzPattern {
    on_ms: u64,
    off_ms: u64,
    /// When the current phase (on or off) started.
    phase_start: Instant,
    /// True when the motor is currently on.
    motor_on: bool,
}

impl BuzzPattern {
    /// Start a new buzz pattern. The motor should be driven ON
    /// immediately by the caller (matching the initial `motor_on =
    /// true` state recorded here).
    pub fn start(on_ms: u64, off_ms: u64, now: Instant) -> Self {
        Self { on_ms, off_ms, phase_start: now, motor_on: true }
    }

    /// Advance the state machine. If the current phase has
    /// elapsed, flip the phase and return [`BuzzAction::TurnOn`]
    /// or [`BuzzAction::TurnOff`] so the caller drives the motor
    /// accordingly. Otherwise returns [`BuzzAction::None`].
    pub fn tick(&mut self, now: Instant) -> BuzzAction {
        let elapsed_ms = now.duration_since(self.phase_start).as_millis();
        let phase_duration = if self.motor_on { self.on_ms } else { self.off_ms };
        if elapsed_ms < phase_duration {
            return BuzzAction::None;
        }
        self.motor_on = !self.motor_on;
        self.phase_start = now;
        if self.motor_on {
            BuzzAction::TurnOn
        } else {
            BuzzAction::TurnOff
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: construct an Instant at t_ms from the start anchor.
    // Can't call `Instant::now()` in host tests - embassy-time
    // requires a registered time driver which only exists on
    // embedded. Build from `Instant::from_millis` instead.
    fn at(t_ms: u64) -> Instant {
        Instant::from_millis(t_ms)
    }

    #[test]
    fn no_toggle_before_on_phase_elapses() {
        let mut p = BuzzPattern::start(100, 400, at(0));
        assert_eq!(p.tick(at(50)), BuzzAction::None);
        assert_eq!(p.tick(at(99)), BuzzAction::None);
    }

    #[test]
    fn turns_off_at_end_of_on_phase() {
        let mut p = BuzzPattern::start(100, 400, at(0));
        assert_eq!(p.tick(at(100)), BuzzAction::TurnOff);
        // After turning off, we're in the off phase; another tick
        // at the same instant should be a no-op (off phase is
        // 400 ms long).
        assert_eq!(p.tick(at(100)), BuzzAction::None);
    }

    #[test]
    fn cycles_on_off_on_off() {
        let mut p = BuzzPattern::start(100, 400, at(0));
        assert_eq!(p.tick(at(100)), BuzzAction::TurnOff); // on phase done
        assert_eq!(p.tick(at(499)), BuzzAction::None);    // still in off
        assert_eq!(p.tick(at(500)), BuzzAction::TurnOn);  // off phase done
        assert_eq!(p.tick(at(599)), BuzzAction::None);
        assert_eq!(p.tick(at(600)), BuzzAction::TurnOff); // on again
    }
}
