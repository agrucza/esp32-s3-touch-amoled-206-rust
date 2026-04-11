/// All system-level events produced by polling subsystems.
///
/// The main loop collects these each tick, then dispatches them
/// to handlers. This separates event production (polling) from
/// event handling (actions/state changes).
#[allow(dead_code)] // event payload fields are part of the public API
#[derive(Debug, Clone)]
pub enum SystemEvent {
    // -- Input --
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
    /// Swipe gesture completed on release
    Swipe { dir: SwipeDir, region: SwipeRegion },

    // -- State changes --
    /// Clock minute changed
    MinuteChanged,
    /// Battery percentage changed
    BatteryChanged { percent: u8 },
}

/// Direction of a swipe gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwipeDir {
    Left,
    Right,
    Up,
    Down,
}

/// Screen region where a swipe gesture started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwipeRegion {
    /// Gesture started in the header band (above CONTENT_TOP).
    Header,
    /// Gesture started in the content band.
    Content,
    /// Gesture started in the footer band (below CONTENT_BOTTOM).
    Footer,
}
