//! AXP2101 interrupt sources and status.
//!
//! The AXP2101 has a single IRQ pin (active-low) and 24 interrupt sources
//! spread across three 8-bit registers:
//!
//!   REG 40h / 48h  — gauge and battery temperature
//!   REG 41h / 49h  — VBUS, battery insert/remove, POWERON button
//!   REG 42h / 4Ah  — charger and over-voltage/current protection
//!
//! Enable registers (40h–42h): write 1 to enable an interrupt source.
//! Status registers (48h–4Ah): read to see what fired; write 1 to clear (RW1C).
//!
//! ## How to use
//!
//! 1. Call `Pmu::read_interrupts()` to get an `InterruptStatus`.
//! 2. Check individual sources with `status.is_active(InterruptSource::VbusInsert)`.
//! 3. Call `Pmu::clear_interrupts(&status)` to acknowledge (write 1 back to clear).
//!
//! ## Combined 24-bit representation
//!
//! Internally all three status bytes are combined into a single `u32`:
//!
//!   bits  0– 7  →  REG 48h (IRQ Status 0)
//!   bits  8–15  →  REG 49h (IRQ Status 1)
//!   bits 16–23  →  REG 4Ah (IRQ Status 2)
//!
//! `InterruptSource::mask()` returns the bit position within this 24-bit word.

/// Every interrupt source the AXP2101 can generate.
///
/// The names match the signal names in the AXP2101 datasheet (section 6.13.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptSource {
    // ---- REG 48h (IRQ Status 0) — gauge and battery temperature ------------
    /// Battery under-temperature in work mode (bwut_irq) — REG48 bit 0.
    BatteryUnderTempWork,
    /// Battery over-temperature in work mode (bwot_irq) — REG48 bit 1.
    BatteryOverTempWork,
    /// Battery under-temperature in charge mode (bcut_irq) — REG48 bit 2.
    BatteryUnderTempCharge,
    /// Battery over-temperature in charge mode (bcot_irq) — REG48 bit 3.
    BatteryOverTempCharge,
    /// Fuel gauge produced a new SOC value (lowsoc_irq) — REG48 bit 4.
    GaugeNewSoc,
    /// Gauge watchdog timed out (gwdt_irq) — REG48 bit 5.
    GaugeWatchdogTimeout,
    /// SOC dropped to warning level 1 (socwl1_irq) — REG48 bit 6.
    SocWarningLevel1,
    /// SOC dropped to warning level 2 (socwl2_irq) — REG48 bit 7.
    SocWarningLevel2,

    // ---- REG 49h (IRQ Status 1) — VBUS, battery, POWERON button ------------
    /// POWERON positive edge detected (ponpe_irq) — REG49 bit 0.
    PowerOnPositiveEdge,
    /// POWERON negative edge detected (ponne_irq) — REG49 bit 1.
    PowerOnNegativeEdge,
    /// POWERON button held for long-press duration (ponlp_irq) — REG49 bit 2.
    PowerOnLongPress,
    /// POWERON button short press (ponsp_irq) — REG49 bit 3.
    PowerOnShortPress,
    /// Battery removed from connector (bremove_irq) — REG49 bit 4.
    BatteryRemove,
    /// Battery inserted into connector (binsert_irq) — REG49 bit 5.
    BatteryInsert,
    /// VBUS removed (vremove_irq) — REG49 bit 6.
    VbusRemove,
    /// VBUS inserted and good (vinsert_irq) — REG49 bit 7.
    VbusInsert,

    // ---- REG 4Ah (IRQ Status 2) — charger and protection -------------------
    /// Battery over-voltage protection triggered (bovp_irq) — REG4A bit 0.
    BatteryOverVoltage,
    /// Charger safety timer 1 or 2 expired (chgte_irq) — REG4A bit 1.
    ChargerSafetyTimerExpire,
    /// Die over-temperature level 1 (dotl1_irq) — REG4A bit 2.
    DieOverTempLevel1,
    /// Battery charging started (chgst_irq) — REG4A bit 3.
    ChargerStart,
    /// Battery charging finished (chgdn_irq) — REG4A bit 4.
    BatteryChargeDone,
    // bit 5 is reserved in the hardware — no enum variant
    /// LDO output over-current (ldooc_irq) — REG4A bit 6.
    LdoOverCurrent,
    /// System watchdog expired (wdexp_irq) — REG4A bit 7.
    WatchdogExpire,
}

impl InterruptSource {
    /// Returns the bitmask for this source within the combined 24-bit status word
    /// produced by `InterruptStatus::new(reg48, reg49, reg4a)`.
    pub const fn mask(self) -> u32 {
        match self {
            // REG 48h — bits 0–7
            Self::BatteryUnderTempWork    => 1 << 0,
            Self::BatteryOverTempWork     => 1 << 1,
            Self::BatteryUnderTempCharge  => 1 << 2,
            Self::BatteryOverTempCharge   => 1 << 3,
            Self::GaugeNewSoc             => 1 << 4,
            Self::GaugeWatchdogTimeout    => 1 << 5,
            Self::SocWarningLevel1        => 1 << 6,
            Self::SocWarningLevel2        => 1 << 7,
            // REG 49h — bits 8–15
            Self::PowerOnPositiveEdge     => 1 << 8,
            Self::PowerOnNegativeEdge     => 1 << 9,
            Self::PowerOnLongPress        => 1 << 10,
            Self::PowerOnShortPress       => 1 << 11,
            Self::BatteryRemove           => 1 << 12,
            Self::BatteryInsert           => 1 << 13,
            Self::VbusRemove              => 1 << 14,
            Self::VbusInsert              => 1 << 15,
            // REG 4Ah — bits 16–23
            Self::BatteryOverVoltage      => 1 << 16,
            Self::ChargerSafetyTimerExpire=> 1 << 17,
            Self::DieOverTempLevel1       => 1 << 18,
            Self::ChargerStart            => 1 << 19,
            Self::BatteryChargeDone       => 1 << 20,
            Self::LdoOverCurrent          => 1 << 22, // bit 21 is reserved
            Self::WatchdogExpire          => 1 << 23,
        }
    }
}

/// Snapshot of all three IRQ Status registers combined into one value.
///
/// No heap allocation — check individual sources with [`is_active`].
///
/// [`is_active`]: InterruptStatus::is_active
#[derive(Debug, Clone, Copy, Default)]
pub struct InterruptStatus {
    /// Combined 24-bit status: bits 0–7 = REG48, 8–15 = REG49, 16–23 = REG4A.
    pub raw: u32,
}

impl InterruptStatus {
    /// Build a status snapshot from the three raw register bytes read from hardware.
    pub fn new(reg48: u8, reg49: u8, reg4a: u8) -> Self {
        Self {
            raw: (reg48 as u32) | ((reg49 as u32) << 8) | ((reg4a as u32) << 16),
        }
    }

    /// Returns `true` if the given interrupt source is active.
    pub fn is_active(self, source: InterruptSource) -> bool {
        (self.raw & source.mask()) != 0
    }

    /// Returns `true` if no interrupts are currently active.
    pub fn is_empty(self) -> bool {
        self.raw == 0
    }

    /// Extract the raw byte for REG 48h, for writing back to clear those bits.
    pub fn reg48_byte(self) -> u8 {
        (self.raw & 0xFF) as u8
    }

    /// Extract the raw byte for REG 49h.
    pub fn reg49_byte(self) -> u8 {
        ((self.raw >> 8) & 0xFF) as u8
    }

    /// Extract the raw byte for REG 4Ah.
    pub fn reg4a_byte(self) -> u8 {
        ((self.raw >> 16) & 0xFF) as u8
    }
}

/// Interrupt enable configuration — which sources should trigger the IRQ pin.
///
/// Build one of these, then pass it to `Pmu::configure_interrupts()`.
///
/// Default: all interrupts disabled (safe starting point — opt in explicitly).
#[derive(Debug, Clone, Copy, Default)]
pub struct InterruptConfig {
    /// Combined 24-bit enable mask (same layout as `InterruptStatus::raw`).
    pub mask: u32,
}

impl InterruptConfig {
    /// Create a config with all interrupts disabled.
    pub fn none() -> Self {
        Self { mask: 0 }
    }

    /// Enable one interrupt source (builder pattern — can be chained).
    pub fn enable(mut self, source: InterruptSource) -> Self {
        self.mask |= source.mask();
        self
    }

    /// Disable one interrupt source.
    pub fn disable(mut self, source: InterruptSource) -> Self {
        self.mask &= !source.mask();
        self
    }

    /// Raw byte for REG 40h (IRQ Enable 0).
    pub fn reg40_byte(self) -> u8 {
        (self.mask & 0xFF) as u8
    }

    /// Raw byte for REG 41h (IRQ Enable 1).
    pub fn reg41_byte(self) -> u8 {
        ((self.mask >> 8) & 0xFF) as u8
    }

    /// Raw byte for REG 42h (IRQ Enable 2).
    pub fn reg42_byte(self) -> u8 {
        ((self.mask >> 16) & 0xFF) as u8
    }
}
