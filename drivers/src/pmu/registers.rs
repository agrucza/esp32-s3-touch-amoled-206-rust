//! AXP2101 register addresses and bit definitions.
//!
//! All addresses are taken directly from the AXP2101 datasheet
//! (X-power AXP2101 SWcharge V1.0), section 6.13.
//!
//! Naming convention:
//!   REG_*  - register address (u8)
//!   Bit constants are provided as masks (1 << bit_position) in sub-modules.

// =============================================================================
// Chip ID register (REG 03h)
// =============================================================================

/// Chip ID register address (REG 03h, read-only).
///
/// Bit layout:
///   7:6  chip_id_h    - upper ID bits (always 0b01 for AXP2101)
///   5:4  chip_version - 00=A, 01=B
///   3:0  chip_id_l    - lower ID bits
///
/// Observed on ESP32-S3-Touch-AMOLED-2.06: 0x4A (version A).
/// Verification uses only chip_id_h (bits 7:6 = 0b01) to be robust
/// across chip versions and revisions.
pub const CHIP_ID: u8 = 0x03;

/// Mask for bits 7:6 (chip_id_h) - the stable part of the chip identity.
pub const CHIP_ID_MASK:  u8 = 0b1100_0000;
/// Expected value of (raw_byte & CHIP_ID_MASK) for any AXP2101.
pub const CHIP_ID_VALUE: u8 = 0b0100_0000; // chip_id_h = 0b01

// =============================================================================
// Status registers (read-only)
// =============================================================================

/// PMU Status 1 (REG 00h) - power-path and battery state.
pub const REG_PMU_STATUS1: u8 = 0x00;
/// PMU Status 2 (REG 01h) - charging state and current direction.
pub const REG_PMU_STATUS2: u8 = 0x01;

// =============================================================================
// Data buffer (non-volatile scratch space across soft-resets)
// =============================================================================

pub const REG_DATA_BUF0: u8 = 0x04;
pub const REG_DATA_BUF1: u8 = 0x05;
pub const REG_DATA_BUF2: u8 = 0x06;
pub const REG_DATA_BUF3: u8 = 0x07;

// =============================================================================
// Power management & system configuration
// =============================================================================

/// PMU common configuration (REG 10h).
/// Bit 0 = soft power-off, bit 1 = SoC restart, bit 2 = 16s-press shutdown,
/// bit 3 = PWROK restart, bit 5 = internal off-discharge enable.
pub const REG_PMU_CFG: u8 = 0x10;

/// BATFET control (REG 12h).
/// Bit 3 = BATFET enable during power-off + battery-only.
pub const REG_BATFET_CFG: u8 = 0x12;

/// Die temperature control (REG 13h).
/// Bits 2:1 = OTP level (00=115°C … 10=135°C), bit 0 = detect enable.
pub const REG_DIE_TEMP_CFG: u8 = 0x13;

/// Minimum system voltage (REG 14h).
/// Bits 2:0 → 3.2 + N×0.1 V  (default 101b = 3.7 V).
pub const REG_SYS_VMIN: u8 = 0x14;

/// Input voltage limit - VINDPM (REG 15h).
/// Bits 3:0 → 3.88 + N×0.08 V  (default 0110b = 4.36 V).
pub const REG_VINDPM: u8 = 0x15;

/// Input current limit (REG 16h).
/// Bits 2:0 → 000=100 mA … 101=2000 mA  (default 100b = 1500 mA).
pub const REG_ILIM: u8 = 0x16;

// =============================================================================
// Fuel gauge
// =============================================================================

/// Fuel gauge reset control (REG 17h).
pub const REG_GAUGE_RESET: u8 = 0x17;

/// Charger / gauge / watchdog enable (REG 18h).
/// Bit 3 = gauge enable, bit 2 = button-battery charge, bit 1 = cell charge,
/// bit 0 = watchdog enable.
pub const REG_CHARGER_GAUGE_WDT_EN: u8 = 0x18;

/// Watchdog control (REG 19h).
/// Bits 5:4 = reset action, bits 2:0 = timeout (000=1s … 111=128s).
pub const REG_WDT_CFG: u8 = 0x19;

/// Low battery warning thresholds (REG 1Ah).
/// Bits 7:4 = level2 (5–20%), bits 3:0 = level1 (0–15%).
pub const REG_LOWBAT_WARN: u8 = 0x1A;

/// GPIO1 output configuration (REG 1Bh).
pub const REG_GPIO1_CFG: u8 = 0x1B;

// =============================================================================
// Power-on / power-off status and control
// =============================================================================

/// PWRON status - sources that caused the last power-on (REG 20h, read-only).
pub const REG_PWRON_STATUS: u8 = 0x20;

/// PWROFF status - sources that caused the last power-off (REG 21h, read-only).
pub const REG_PWROFF_STATUS: u8 = 0x21;

/// PWROFF enable (REG 22h).
/// Bit 2 = die OTP level2 as power-off source, bit 1 = long-press off enable.
pub const REG_PWROFF_EN: u8 = 0x22;

/// DCDC OVP/UVP power-off control (REG 23h).
pub const REG_DCDC_OVP_UVP: u8 = 0x23;

/// Vsys voltage for power-off threshold (REG 24h).
/// Bits 2:0 → 2.6 + N×0.1 V.
pub const REG_VSYS_PWROFF: u8 = 0x24;

/// PWROK and power-off sequence control (REG 25h).
pub const REG_PWROK_CFG: u8 = 0x25;

/// Sleep and wakeup control (REG 26h).
/// Bit 0 = sleep enable, bit 1 = wakeup enable, bit 4 = IRQ-pin wakeup.
pub const REG_SLEEP_WAKEUP: u8 = 0x26;

/// IRQ / off / on level timing (REG 27h).
/// Bits 5:4 = IRQ level, bits 3:2 = off level, bits 1:0 = on level.
pub const REG_LEVEL_CFG: u8 = 0x27;

// =============================================================================
// Fast power-on sequencing (REG 28h–2Bh)
// =============================================================================

pub const REG_FAST_PWRON0: u8 = 0x28;
pub const REG_FAST_PWRON1: u8 = 0x29;
pub const REG_FAST_PWRON2: u8 = 0x2A;
pub const REG_FAST_PWRON3: u8 = 0x2B;

// =============================================================================
// ADC
// =============================================================================

/// ADC channel enable (REG 30h).
/// Bit 0 = battery voltage, bit 1 = TS pin, bit 2 = VBUS, bit 3 = Vsys,
/// bit 4 = die temperature, bit 5 = general-purpose ADC.
pub const REG_ADC_EN: u8 = 0x30;

/// Battery voltage high byte (REG 34h), bits 5:0 = vbat[13:8].
pub const REG_VBAT_H: u8 = 0x34;
/// Battery voltage low byte (REG 35h), bits 7:0 = vbat[7:0].
pub const REG_VBAT_L: u8 = 0x35;

/// TS voltage high byte (REG 36h), bits 5:0 = ts[13:8].
pub const REG_TS_H: u8 = 0x36;
/// TS voltage low byte (REG 37h).
pub const REG_TS_L: u8 = 0x37;

/// VBUS voltage high byte (REG 38h), bits 5:0 = vbus[13:8].
pub const REG_VBUS_H: u8 = 0x38;
/// VBUS voltage low byte (REG 39h).
pub const REG_VBUS_L: u8 = 0x39;

/// System voltage high byte (REG 3Ah), bits 5:0 = vsys[13:8].
pub const REG_VSYS_H: u8 = 0x3A;
/// System voltage low byte (REG 3Bh).
pub const REG_VSYS_L: u8 = 0x3B;

/// Die temperature high byte (REG 3Ch), bits 5:0 = tdie[13:8].
pub const REG_TDIE_H: u8 = 0x3C;
/// Die temperature low byte (REG 3Dh).
pub const REG_TDIE_L: u8 = 0x3D;

// =============================================================================
// IRQ - enable registers (REG 40h–42h)
// Writing 1 to a bit enables that interrupt source.
// =============================================================================

/// IRQ Enable 0 (REG 40h) - gauge and battery temperature interrupts.
///
/// | Bit | Source                                        |
/// |-----|-----------------------------------------------|
/// |  7  | SOC drop to warning level 2 (socwl2_irq)     |
/// |  6  | SOC drop to warning level 1 (socwl1_irq)     |
/// |  5  | Gauge watchdog timeout      (gwdt_irq)        |
/// |  4  | Gauge new SOC               (lowsoc_irq)      |
/// |  3  | Battery over-temp in charge (bcot_irq)        |
/// |  2  | Battery under-temp in charge(bcut_irq)        |
/// |  1  | Battery over-temp in work   (bwot_irq)        |
/// |  0  | Battery under-temp in work  (bwut_irq)        |
pub const REG_IRQ_EN0: u8 = 0x40;

/// IRQ Enable 1 (REG 41h) - VBUS, battery insert/remove, POWERON button.
///
/// | Bit | Source                                        |
/// |-----|-----------------------------------------------|
/// |  7  | VBUS insert                 (vinsert_irq)     |
/// |  6  | VBUS remove                 (vremove_irq)     |
/// |  5  | Battery insert              (binsert_irq)     |
/// |  4  | Battery remove              (bremove_irq)     |
/// |  3  | POWERON short press         (ponsp_irq)       |
/// |  2  | POWERON long press          (ponlp_irq)       |
/// |  1  | POWERON negative edge       (ponne_irq)       |
/// |  0  | POWERON positive edge       (ponpe_irq)       |
pub const REG_IRQ_EN1: u8 = 0x41;

/// IRQ Enable 2 (REG 42h) - charger and protection interrupts.
///
/// | Bit | Source                                        |
/// |-----|-----------------------------------------------|
/// |  7  | Watchdog expire             (wdexp_irq)       |
/// |  6  | LDO over-current            (ldooc_irq)       |
/// |  5  | Reserved                                      |
/// |  4  | Battery charge done         (chgdn_irq)       |
/// |  3  | Charger start               (chgst_irq)       |
/// |  2  | Die over-temp level 1       (dotl1_irq)       |
/// |  1  | Charger safety timer expire (chgte_irq)       |
/// |  0  | Battery over-voltage prot.  (bovp_irq)        |
pub const REG_IRQ_EN2: u8 = 0x42;

// =============================================================================
// IRQ - status registers (REG 48h–4Ah)
// Read to check which interrupt fired. Write 1 to a bit to clear it (RW1C).
// Layout mirrors the enable registers above.
// =============================================================================

/// IRQ Status 0 (REG 48h) - gauge and battery temperature events.
pub const REG_IRQ_STATUS0: u8 = 0x48;
/// IRQ Status 1 (REG 49h) - VBUS, battery insert/remove, POWERON button events.
pub const REG_IRQ_STATUS1: u8 = 0x49;
/// IRQ Status 2 (REG 4Ah) - charger and protection events.
pub const REG_IRQ_STATUS2: u8 = 0x4A;

// =============================================================================
// TS (temperature sensor) pin configuration (REG 50h–57h)
// =============================================================================

pub const REG_TS_CFG:       u8 = 0x50;
pub const REG_TS_HYSL2H:    u8 = 0x52;
pub const REG_TS_HYSH2L:    u8 = 0x53;
pub const REG_VLTF_CHG:     u8 = 0x54;
pub const REG_VHTF_CHG:     u8 = 0x55;
pub const REG_VLTF_WORK:    u8 = 0x56;
pub const REG_VHTF_WORK:    u8 = 0x57;

// =============================================================================
// JEITA standard (REG 58h–5Bh)
// =============================================================================

pub const REG_JEITA_EN:     u8 = 0x58;
pub const REG_JEITA_CV_CFG: u8 = 0x59;
pub const REG_JEITA_COOL:   u8 = 0x5A;
pub const REG_JEITA_WARM:   u8 = 0x5B;

// =============================================================================
// Charger configuration (REG 61h–6Ah)
// =============================================================================

/// Pre-charge current limit (REG 61h). Bits 3:0 → 25×N mA.
pub const REG_IPRECHG:      u8 = 0x61;
/// Constant-current charge current (REG 62h). Bits 4:0, see datasheet table.
pub const REG_ICC:          u8 = 0x62;
/// Termination current and enable (REG 63h). Bit 4 = term enable, bits 3:0 = 25×N mA.
pub const REG_ITERM:        u8 = 0x63;
/// CV charge voltage (REG 64h). Bits 2:0 → see table (001=4.0V … 101=4.4V).
pub const REG_CV_VOLT:      u8 = 0x64;
/// Thermal regulation threshold (REG 65h). Bits 1:0 → 00=60°C … 11=120°C.
pub const REG_THERMAL_REG:  u8 = 0x65;
/// Charger safety timer (REG 67h).
pub const REG_CHG_TIMER:    u8 = 0x67;
/// Battery detection enable (REG 68h). Bit 0 = enable.
pub const REG_BAT_DET:      u8 = 0x68;
/// Charge LED control (REG 69h).
pub const REG_CHGLED:       u8 = 0x69;
/// Button battery termination voltage (REG 6Ah). Bits 2:0 → 2.6 + N×0.1 V.
pub const REG_BTN_BAT_VTERM: u8 = 0x6A;

// =============================================================================
// DCDC converters (REG 80h–86h)
// =============================================================================

/// DCDC on/off and DVM control (REG 80h).
/// Bits 3:0 = DCDC4–DCDC1 enable.
pub const REG_DCDC_EN:      u8 = 0x80;
/// DCDC PWM/PFM and frequency spread (REG 81h).
pub const REG_DCDC_PWM:     u8 = 0x81;
/// DCDC1 voltage (REG 82h). Bits 4:0 → 1.5 + N×0.1 V.
pub const REG_DCDC1_VOLT:   u8 = 0x82;
/// DCDC2 voltage (REG 83h). Bits 6:0, see datasheet.
pub const REG_DCDC2_VOLT:   u8 = 0x83;
/// DCDC3 voltage (REG 84h). Bits 6:0, see datasheet.
pub const REG_DCDC3_VOLT:   u8 = 0x84;
/// DCDC4 voltage (REG 85h). Bits 6:0, see datasheet.
pub const REG_DCDC4_VOLT:   u8 = 0x85;

// =============================================================================
// LDO regulators (REG 90h–9Ah)
// =============================================================================

/// LDO ON/OFF control 0 (REG 90h).
///
/// | Bit | LDO     |
/// |-----|---------|
/// |  7  | DLDO1   |
/// |  6  | CPUSLDO |
/// |  5  | BLDO2   |
/// |  4  | BLDO1   |
/// |  3  | ALDO4   |
/// |  2  | ALDO3   |
/// |  1  | ALDO2   |
/// |  0  | ALDO1   |
pub const REG_LDO_EN0:      u8 = 0x90;

/// LDO ON/OFF control 1 (REG 91h). Bit 0 = DLDO2 enable.
pub const REG_LDO_EN1:      u8 = 0x91;

/// ALDO1 voltage (REG 92h). Bits 4:0 → 0.5 + N×0.1 V.
pub const REG_ALDO1_VOLT:   u8 = 0x92;
/// ALDO2 voltage (REG 93h).
pub const REG_ALDO2_VOLT:   u8 = 0x93;
/// ALDO3 voltage (REG 94h).
pub const REG_ALDO3_VOLT:   u8 = 0x94;
/// ALDO4 voltage (REG 95h).
pub const REG_ALDO4_VOLT:   u8 = 0x95;
/// BLDO1 voltage (REG 96h).
pub const REG_BLDO1_VOLT:   u8 = 0x96;
/// BLDO2 voltage (REG 97h).
pub const REG_BLDO2_VOLT:   u8 = 0x97;
/// CPUSLDO voltage (REG 98h). Bits 4:0 → 0.5 + N×0.05 V.
pub const REG_CPUSLDO_VOLT: u8 = 0x98;
/// DLDO1 voltage (REG 99h). Bits 4:0 → 0.5 + N×0.1 V.
pub const REG_DLDO1_VOLT:   u8 = 0x99;
/// DLDO2 voltage (REG 9Ah). Bits 4:0 → 0.5 + N×0.05 V.
pub const REG_DLDO2_VOLT:   u8 = 0x9A;

// =============================================================================
// Fuel gauge / battery percentage (REG A1h–A4h)
// =============================================================================

/// Battery model parameter ROM (REG A1h, read-only).
pub const REG_BAT_PARAM:    u8 = 0xA1;
/// Fuel gauge control (REG A2h).
pub const REG_GAUGE_CFG:    u8 = 0xA2;
/// Battery percentage, 0–100 % (REG A4h, read-only).
pub const REG_BAT_PERCENT:  u8 = 0xA4;

// =============================================================================
// Convenience: bitmask constants for LDO enable register (REG 90h)
// =============================================================================
pub mod ldo_en0 {
    pub const ALDO1:   u8 = 1 << 0;
    pub const ALDO2:   u8 = 1 << 1;
    pub const ALDO3:   u8 = 1 << 2;
    pub const ALDO4:   u8 = 1 << 3;
    pub const BLDO1:   u8 = 1 << 4;
    pub const BLDO2:   u8 = 1 << 5;
    pub const CPUSLDO: u8 = 1 << 6;
    pub const DLDO1:   u8 = 1 << 7;
}

// =============================================================================
// Convenience: bitmask constants for ADC enable register (REG 30h)
// =============================================================================
pub mod adc_en {
    pub const BAT_VOLT:  u8 = 1 << 0;
    pub const TS_PIN:    u8 = 1 << 1;
    pub const VBUS_VOLT: u8 = 1 << 2;
    pub const VSYS_VOLT: u8 = 1 << 3;
    pub const DIE_TEMP:  u8 = 1 << 4;
    pub const GPADC:     u8 = 1 << 5;
}
