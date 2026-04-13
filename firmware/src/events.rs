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
    /// Touch screen pressed at coordinates. Fires on initial contact
    /// and during drag with updated positions; use for live
    /// drag-tracking (sliders, scrolling). **Do not treat a single
    /// `TouchPressed` as a click** - use `Tap` for that.
    TouchPressed { x: u16, y: u16 },
    /// Touch screen released (finger lifted).
    TouchReleased,
    /// Tap gesture: press and release with minimal movement. Fires on
    /// release at the start position of the press. This is what
    /// screens should use for "click" semantics.
    Tap { x: u16, y: u16 },
    /// Swipe gesture completed on release.
    Swipe { dir: SwipeDir, region: SwipeRegion },

    // -- State changes --
    /// Clock minute changed
    MinuteChanged,
    /// Battery percentage changed
    BatteryChanged { percent: u8 },
    /// IMU Wake-on-Motion fired while the system was sleeping.
    /// Injected by the sleep handler on the tick after the INT1
    /// rising edge so the main event loop can react to it like
    /// any other wake source.
    WakeOnMotion,
}

/// Direction of a swipe gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwipeDir {
    Left,
    Right,
    Up,
    Down,
}

/// Screen region where a swipe gesture *started*, classified by the
/// system gesture edge zones (see `theme::EDGE_GESTURE_ZONE`).
///
/// Top and Bottom are the edge-gesture zones reserved for
/// system-level actions (e.g. swipe-down-from-top opens the panel).
/// Content is the middle band that belongs to the active screen.
/// Both edges are separate variants so handlers can distinguish
/// "swipe down from top" vs "swipe up from bottom" if they need to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwipeRegion {
    /// Gesture started within `EDGE_GESTURE_ZONE` pixels of the top
    /// edge.
    Top,
    /// Gesture started in the central content band.
    Content,
    /// Gesture started within `EDGE_GESTURE_ZONE` pixels of the bottom
    /// edge.
    Bottom,
}
