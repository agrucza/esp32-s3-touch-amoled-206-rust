//! Snapshot data types produced by peripheral tasks and consumed
//! by the UI.
//!
//! These were originally defined inside each task file in
//! `firmware/src/system/tasks/`, but they're pure value types
//! shared by both sides and belong in `app-core` where the UI can
//! reach them without pulling in hardware. The tasks re-export
//! them via `pub use` so existing task-path imports keep working.

use drivers::imu::ImuData;
use drivers::pmu::{ChargeVoltage, ChargerPhase, CurrentDirection, InputCurrentLimit};
use drivers::rtc::DateTime as RtcDateTime;

// ============================================================================
// TimeData - calendar time of day, consumed by clock-style screens.
// ============================================================================

/// Calendar time of day. Defaults to an arbitrary recent date so
/// screens have something reasonable to render before the first
/// RTC read.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // `second` is read by future screens (seconds face)
pub struct TimeData {
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

impl Default for TimeData {
    fn default() -> Self {
        Self { hour: 0, minute: 0, second: 0, year: 2026, month: 1, day: 1 }
    }
}

impl From<&RtcDateTime> for TimeData {
    fn from(dt: &RtcDateTime) -> Self {
        Self {
            hour: dt.hour,
            minute: dt.minute,
            second: dt.second,
            year: dt.year,
            month: dt.month,
            day: dt.day,
        }
    }
}

// ============================================================================
// PowerData - flat snapshot of everything the UI wants from the PMU.
// ============================================================================

/// Flat snapshot of everything the UI wants from the PMU, so
/// screens can read `data.power.vbus_good` directly without
/// going through a nested struct. Fields that come from an I2C
/// read that can fail are `Option<_>`; status flags default to
/// their inactive state when the read fails (screens treat that
/// as "nothing is happening").
#[derive(Debug, Clone, Copy, Default)]
#[allow(dead_code)]
pub struct PowerData {
    // --- Battery ---
    pub battery_percent: Option<u8>,
    pub battery_voltage_mv: Option<u16>,

    // --- Power path (from PMU status register 1) ---
    /// VBUS is present and above the VBUS good threshold.
    pub vbus_good: bool,
    /// BATFET is on (battery connected to the power path).
    pub batfet_active: bool,
    /// Battery is detected by the charger.
    pub battery_present: bool,
    /// Battery is in active (non-sleep) mode.
    pub battery_active: bool,
    /// Die is in thermal regulation (charging current reduced).
    pub thermal_active: bool,
    /// Input current limit regulation is active.
    pub current_limit_active: bool,

    // --- Charger state (from PMU status register 2) ---
    /// Battery current direction (standby / charging / discharging).
    pub current_direction: CurrentDirection,
    /// Charger phase (tri-charge / pre-charge / CC / CV / done / not charging).
    pub charger_phase: ChargerPhase,
    /// System is powered on (always true while we're running).
    pub system_on: bool,
    /// VINDPM regulation is active (input voltage at limit).
    pub vindpm_active: bool,

    // --- ADC readings ---
    pub vbus_voltage_mv: Option<u16>,
    pub system_voltage_mv: Option<u16>,
    pub die_temperature_raw: Option<u16>,

    // --- Charger config (typically static, read once to verify) ---
    pub charge_current_ma: Option<u16>,
    pub charge_voltage: Option<ChargeVoltage>,
    pub input_current_limit: Option<InputCurrentLimit>,
    pub input_voltage_limit_mv: Option<u16>,
}

// ============================================================================
// MotionData - IMU sample, consumed by the status screen motion panel.
// ============================================================================

/// Snapshot of raw IMU axes + die temperature. Defaults to zeros
/// so screens have something to render before the first read.
#[derive(Debug, Clone, Copy, Default)]
#[allow(dead_code)]
pub struct MotionData {
    pub accel_x: i16,
    pub accel_y: i16,
    pub accel_z: i16,
    pub gyro_x: i16,
    pub gyro_y: i16,
    pub gyro_z: i16,
    pub temp_raw: i16,
}

impl From<&ImuData> for MotionData {
    fn from(d: &ImuData) -> Self {
        Self {
            accel_x: d.accel_x,
            accel_y: d.accel_y,
            accel_z: d.accel_z,
            gyro_x: d.gyro_x,
            gyro_y: d.gyro_y,
            gyro_z: d.gyro_z,
            temp_raw: d.temp_raw,
        }
    }
}

// ============================================================================
// TouchData - current touch point, or `None` fields if idle.
// ============================================================================

/// Current touch point. Both fields are `None` when no finger is
/// down. Updated incrementally from `TouchPressed` / `TouchReleased`
/// events by the main event handler - no I2C reads required.
#[derive(Debug, Clone, Copy, Default)]
pub struct TouchData {
    pub x: Option<u16>,
    pub y: Option<u16>,
}

// ============================================================================
// NvsUsage - flash-backed config-store occupancy, for the settings screen.
// ============================================================================

/// Summary of the firmware's flash-backed config store. Updated
/// at boot and after every save via
/// [`crate::events::SystemEvent::NvsUsageUpdated`].
///
/// `total_bytes` is the size of the region reserved for config
/// (64 KB by default, defined by
/// `firmware::system::nvs::FLASH_REGION_SIZE`). `records` is the
/// number of live (latest-per-key) entries.
///
/// Exact on-flash byte usage isn't tracked - sequential-storage
/// doesn't expose record sizes through the public iterator API,
/// and append-only wear-leveling makes "bytes used" a fuzzy
/// number anyway. Record count is what a user actually cares
/// about ("am I running out?").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NvsUsage {
    pub records: u32,
    pub total_bytes: u32,
}
