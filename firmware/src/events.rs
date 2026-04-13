use drivers::pmu::{ChargerPhase, CurrentDirection};

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

    // -- Time --
    /// Half-minute clock tick - emitted when the PCF85063
    /// half-minute interrupt pulses INT# low at second=0 and
    /// second=30 of every minute. Drives time-display redraws.
    HalfMinuteChanged,
    /// RTC alarm fired (set via `Rtc::set_alarm`). The RTC task
    /// reads Control_2 on INT# fall and clears the alarm flag
    /// before emitting this event.
    AlarmFired,
    /// RTC countdown timer expired (set via `Rtc::set_timer`).
    TimerExpired,

    // -- Power / battery --
    /// Battery state-of-charge changed. The power task emits this
    /// whenever the fuel-gauge percentage differs from its last
    /// poll; the new value is in the payload.
    BatteryChanged { percent: u8 },
    /// VBUS (USB power) was just plugged in (status1.vbus_good
    /// transitioned false → true).
    VbusInserted,
    /// VBUS (USB power) was just removed (status1.vbus_good
    /// transitioned true → false).
    VbusRemoved,
    /// Charger phase changed (e.g. entered CC, entered CV, charge
    /// done, stopped charging). The new phase is in the payload.
    ChargerPhaseChanged { phase: ChargerPhase },
    /// Battery current direction changed (standby / charging /
    /// discharging). The new direction is in the payload.
    CurrentDirectionChanged { direction: CurrentDirection },

    // -- Motion --
    /// IMU Wake-on-Motion fired while the system was sleeping.
    /// Emitted by the IMU task when GPIO21 goes high. Drives
    /// the wake path on the main loop.
    WakeOnMotion,
    /// Fresh IMU snapshot (accel + gyro + die temperature).
    /// Emitted by the IMU task at a fixed cadence while awake
    /// so screens that display live motion data stay current.
    /// The main loop's handler just replaces
    /// `cached_data.motion` with the payload.
    MotionUpdated {
        data: crate::system::tasks::imu::MotionData,
    },

    // -- Snapshot refreshes --
    /// Fresh RTC snapshot (calendar date + time of day). Emitted
    /// by the RTC task after every INT# fall, so `cached_data.time`
    /// is updated alongside the triggering event
    /// (HalfMinuteChanged / AlarmFired / TimerExpired).
    TimeUpdated {
        data: crate::system::tasks::rtc::TimeData,
    },
    /// Fresh PMU snapshot (battery, charger, ADC channels, ...).
    /// Emitted by the power task every poll interval so
    /// `cached_data.power` stays current without the main loop
    /// ever touching the bus.
    PowerUpdated {
        data: crate::system::tasks::power::PowerData,
    },
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
/// Edge variants are reserved for system-level actions (e.g.
/// swipe-down-from-top opens the panel). Content is the middle
/// band that belongs to the active screen. All four edges are
/// separate variants so handlers can distinguish direction of
/// origin if they need to.
///
/// When a gesture starts in a corner, vertical edges (Top/Bottom)
/// take precedence over horizontal ones (Left/Right).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwipeRegion {
    /// Gesture started within `EDGE_GESTURE_ZONE` pixels of the top
    /// edge.
    Top,
    /// Gesture started within `EDGE_GESTURE_ZONE` pixels of the
    /// bottom edge.
    Bottom,
    /// Gesture started within `EDGE_GESTURE_ZONE` pixels of the left
    /// edge (but not in the top or bottom zones).
    Left,
    /// Gesture started within `EDGE_GESTURE_ZONE` pixels of the right
    /// edge (but not in the top or bottom zones).
    Right,
    /// Gesture started in the central content band.
    Content,
}
