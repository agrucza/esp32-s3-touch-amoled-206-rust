/// All system-level events produced by polling subsystems.
///
/// The main loop collects these each tick, then dispatches them
/// to handlers. This separates event production (polling) from
/// event handling (actions/state changes).
#[derive(Debug, Clone)]
pub enum SystemEvent {
    /// BOOT button pressed (falling edge)
    BootButtonPressed,
    /// Power button short press (from PMU interrupt)
    PowerButtonShort,
    /// Power button long press (from PMU interrupt)
    PowerButtonLong,
    /// Touch screen pressed at coordinates
    TouchPressed { x: u16, y: u16 },
    /// Touch screen released
    TouchReleased,
}
