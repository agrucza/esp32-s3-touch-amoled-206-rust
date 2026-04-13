//! Power state types for the AXP2101 PMU.
//!
//! These types describe power states and configuration - the actual transitions
//! are performed via REG 26h (sleep/wakeup control) in the AXP2101.
//!
//! ## Wakeup sources on the ESP32-S3-Touch-AMOLED-2.06
//!
//! The AXP2101 IRQ output is tied to the `EXIO5` net which the schematic
//! uses as the RTC-VCC power rail - it is NOT routed to any readable
//! signal. AXP2101 interrupts therefore cannot drive any ESP32 GPIO
//! directly and must be polled over I2C. The actual wakeup paths are:
//!
//! | Source       | Path                                            |
//! |--------------|-------------------------------------------------|
//! | Power button | AXP2101 PWRON → CHIP_PU (ESP32 enable, hard reset) |
//! | Touch        | FT3168 INT → GPIO38 (direct ESP32 ext-wakeup)   |
//! | RTC alarm    | PCF85063 INT → GPIO39 (direct ESP32 ext-wakeup) |
//! | IMU motion   | QMI8658 INT1 → GPIO21 (direct ESP32 ext-wakeup) |
//! | VBUS insert  | AXP2101 IRQ → EXIO5 (not usable - see above)    |
//!
//! The `wakeup_sources` field in [`PowerConfig`] configures which AXP2101
//! interrupt sources are enabled in REG 40h–42h, useful for event handling
//! while the system is running via I2C polling (not deep sleep wakeup).
//!
//! Real power savings figures must be measured with a power profiler
//! (e.g. Nordic PPK2) from the battery - the AXP2101 does not expose
//! system current draw via any register.

/// Current or target power state of the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerState {
    /// All rails enabled, system fully active.
    Normal,
    /// Reduced activity - some peripherals may be gated.
    Sleep,
    /// Minimal power - only wakeup sources active.
    DeepSleep,
    /// All LDOs disabled, system off.
    Off,
}

/// Configuration for a power state transition.
///
/// Note: `irq_sources` configures which AXP2101 events assert the IRQ pin
/// (useful for runtime event handling). The AXP2101 IRQ goes to IO expander
/// EXIO5 on this board - it cannot directly wake the ESP32 from deep sleep.
/// Deep sleep wakeup uses GPIO38 (touch), GPIO39 (RTC), and CHIP_PU (button).
#[derive(Debug, Clone)]
pub struct PowerConfig {
    /// Target power state.
    pub state: PowerState,
    /// AXP2101 IRQ enable bitmask - which events assert the IRQ pin.
    /// Build using `InterruptSource::mask()` values OR'd together.
    pub irq_sources: u32,
}

impl Default for PowerConfig {
    fn default() -> Self {
        use super::interrupts::InterruptSource;
        Self {
            state: PowerState::Normal,
            // Enable the most useful runtime events by default.
            irq_sources: InterruptSource::PowerOnShortPress.mask()
                       | InterruptSource::PowerOnLongPress.mask()
                       | InterruptSource::VbusInsert.mask()
                       | InterruptSource::VbusRemove.mask()
                       | InterruptSource::BatteryChargeDone.mask()
                       | InterruptSource::ChargerStart.mask(),
        }
    }
}

/// Snapshot of power-related runtime information readable from the AXP2101.
#[derive(Debug, Clone)]
pub struct PowerInfo {
    /// Current power state (tracked by firmware, not readable from a register).
    pub state: PowerState,
    /// Battery voltage in millivolts (from ADC REG 34h–35h), if ADC is enabled.
    pub battery_voltage_mv: Option<u16>,
    /// Battery state of charge in percent (from REG A4h), if fuel gauge is enabled.
    pub battery_percent: Option<u8>,
    /// System voltage in millivolts (from ADC REG 3Ah–3Bh), if ADC is enabled.
    pub system_voltage_mv: Option<u16>,
    /// Die temperature raw ADC value (from REG 3Ch–3Dh), if ADC is enabled.
    pub die_temp_raw: Option<u16>,
}

impl PowerInfo {
    pub fn new(state: PowerState) -> Self {
        Self {
            state,
            battery_voltage_mv: None,
            battery_percent:     None,
            system_voltage_mv:   None,
            die_temp_raw:        None,
        }
    }
}
