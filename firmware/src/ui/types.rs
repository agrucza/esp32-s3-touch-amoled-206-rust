//! Core UI types - Screen trait, actions, and shared data.

use embedded_graphics::{draw_target::DrawTarget, pixelcolor::Rgb565};

use crate::events::SystemEvent;
use drivers::imu::ImuData;
use drivers::rtc::DateTime;

// -- Screen IDs --------------------------------------------------------------

/// Identifies which screen to switch to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenId {
    Clock,
    Status,
    CornerTest,
    /// The pull-down app picker. Not part of the home-row rotation
    /// (it's reached only via swipe-down-from-header) and constructed
    /// via `ActiveScreen::new_panel(previous)` because it needs
    /// context that plain `new(id)` doesn't provide.
    Panel,
    // Future: Sensors, Settings, ...
}

// -- Actions -----------------------------------------------------------------

/// What a screen wants the system to do after processing an event.
///
/// `SwitchScreen` is currently unused but stays as part of the screen
/// API - screens may want to programmatically navigate (e.g., a
/// settings screen returning to Clock, an alarm firing jumping to a
/// timer screen).
#[allow(dead_code)] // SwitchScreen is reserved for programmatic nav
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Nothing to do.
    None,
    /// Screen content changed, request a display refresh.
    Redraw,
    /// Switch to a different screen.
    SwitchScreen(ScreenId),
    /// Request system shutdown.
    Shutdown,
}

// -- System data snapshot ----------------------------------------------------

/// Read-only snapshot of system state, passed to screens each frame.
///
/// Screens pick what they need from this. Adding new fields here
/// makes them available to all screens without changing the trait.
#[allow(dead_code)] // `second` is read by future screens (e.g. a seconds face)
pub struct SystemData {
    // Time
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub year: u16,
    pub month: u8,
    pub day: u8,

    // IMU
    pub accel_x: i16,
    pub accel_y: i16,
    pub accel_z: i16,
    pub gyro_x: i16,
    pub gyro_y: i16,
    pub gyro_z: i16,
    pub temp_raw: i16,

    // Touch
    pub touch_x: Option<u16>,
    pub touch_y: Option<u16>,

    // Power
    pub battery_percent: Option<u8>,
    pub battery_voltage_mv: Option<u16>,

    // System
    pub tick_count: u32,
}

impl SystemData {
    pub fn from_sensors(
        time: Option<&DateTime>,
        imu: Option<&ImuData>,
        touch: Option<(u16, u16)>,
        battery_percent: Option<u8>,
        battery_voltage_mv: Option<u16>,
        tick_count: u32,
    ) -> Self {
        Self {
            hour:   time.map_or(0, |t| t.hour),
            minute: time.map_or(0, |t| t.minute),
            second: time.map_or(0, |t| t.second),
            year:   time.map_or(2026, |t| t.year),
            month:  time.map_or(1, |t| t.month),
            day:    time.map_or(1, |t| t.day),
            accel_x: imu.map_or(0, |d| d.accel_x),
            accel_y: imu.map_or(0, |d| d.accel_y),
            accel_z: imu.map_or(0, |d| d.accel_z),
            gyro_x:  imu.map_or(0, |d| d.gyro_x),
            gyro_y:  imu.map_or(0, |d| d.gyro_y),
            gyro_z:  imu.map_or(0, |d| d.gyro_z),
            temp_raw: imu.map_or(0, |d| d.temp_raw),
            touch_x: touch.map(|(x, _)| x),
            touch_y: touch.map(|(_, y)| y),
            battery_percent,
            battery_voltage_mv,
            tick_count,
        }
    }
}

// -- Screen trait -------------------------------------------------------------

/// Trait that all UI screens implement.
///
/// Screens are stateful - they can track animations, scroll positions,
/// selection state, etc. The SystemManager doesn't know or care about
/// screen internals.
pub trait Screen {
    /// Render the screen to the display. Called every frame.
    fn render<D: DrawTarget<Color = Rgb565>>(&self, display: &mut D, data: &SystemData);

    /// Handle a system event. Return an Action to tell the manager what to do.
    fn on_event(&mut self, event: &SystemEvent, data: &SystemData) -> Action;
}
