//! board.rs - pin assignments and board-level constants for the
//! Waveshare ESP32-C6-Touch-AMOLED-2.06 (16MB Flash, no PSRAM, no SD).
//!
//! Source: V1.0 schematic, audited block-by-block on 2026-05-12.
//! Mirrors the structure of firmware/src/board.rs (S3 variant). Where a
//! signal exists on both boards, the constant name is identical so
//! shared code can address either board with the same identifier.
//!
//! These constants are a reference table for the hardware layout. They
//! are intentionally kept feature-complete even when nothing currently
//! imports them - any new subsystem that needs a pin number should be
//! able to pull it from here instead of re-deriving it from the
//! schematic.

#![allow(dead_code)]

// =============================================================================
// Display - CO5300 AMOLED, QSPI interface
// Resolution: 410 x 502 px (portrait)
// Same physical panel and same CO5300 driver IC as the S3 variant.
// Display power rail (DSI_PWR_EN) is gated by AXP2101 ALDO2; firmware
// must enable ALDO2 over I2C and wait for the rail to stabilise before
// any QSPI traffic.
// =============================================================================
pub const LCD_SDIO0:  u8 = 1;   // QSPI Data 0 (MOSI)
pub const LCD_SDIO1:  u8 = 2;   // QSPI Data 1
pub const LCD_SDIO2:  u8 = 3;   // QSPI Data 2
pub const LCD_SDIO3:  u8 = 4;   // QSPI Data 3
pub const LCD_SCLK:   u8 = 0;   // Serial clock
pub const LCD_CS:     u8 = 5;   // Chip select (active low)
pub const LCD_RESET:  u8 = 11;  // Reset (active low)
// LCD_TE: not wired to a GPIO on this board. The signal terminates on
// test pad TP14 only. Firmware must drive display updates without TE
// sync; if visible tearing becomes an issue, the CO5300 supports a
// software-poll fallback via its tear-effect status register.

pub const LCD_WIDTH:  u16 = 410;
pub const LCD_HEIGHT: u16 = 502;

// Column / row offset for the CO5300 on this specific panel.
// Same physical panel as the S3 variant -> same offsets.
pub const LCD_COL_OFFSET: u16 = 22;
pub const LCD_ROW_OFFSET: u16 = 0;

// =============================================================================
// Shared I2C bus
// Six devices share this single bus on the C6:
//   FT3168  (touch),     QMI8658 (IMU),    PCF85063 (RTC),
//   AXP2101 (PMU),       ES8311  (codec),  ES7210   (mic ADC).
// All sit on GPIO7 (SCL) and GPIO8 (SDA).
// =============================================================================
pub const I2C_SDA: u8 = 8;
pub const I2C_SCL: u8 = 7;

// =============================================================================
// Touch controller - FT3168 (I2C, shared bus)
// =============================================================================
pub const TOUCH_INT:       u8 = 15;    // Interrupt (active low)
pub const TOUCH_RST:       u8 = 10;    // Reset (active low)
pub const TOUCH_I2C_ADDR:  u8 = 0x38;  // FT3168 default I2C address

// =============================================================================
// IMU - QMI8658 (I2C, shared bus)
// Both interrupt lines are wired on the C6 (the S3 only routes INT1;
// INT2 lives on a test point there). Each goes through a 0 Ohm jumper
// resistor (R3, R4) so either can be depopulated if not needed.
// =============================================================================
pub const IMU_INT1: u8 = 16;
pub const IMU_INT2: u8 = 17;

// =============================================================================
// RTC - PCF85063 (I2C, shared bus)
// The PCF85063's INT̄ output is only brought out to an unpopulated test
// pad on this board - there is no GPIO route. The S3 uses GPIO39 as a
// minute-tick wake source for adaptive sleep; on the C6 that wake path
// must come from the ESP32-C6's internal SysTimer / LP timer instead.
// =============================================================================
// (no RTC_INT constant - signal is unreachable from firmware)

// =============================================================================
// Power management - AXP2101 (I2C, shared bus)
//
// AXP_IRQ is unrouted (terminates on EXIO5 -> VCC-RTC rail, same as
// the S3 board). PMU events must be polled via I2C; no hardware wake
// is possible from the AXP IRQ line.
//
// ALDO2 powers DSI_PWR_EN (display rail). The audio analog supply is
// also AXP-controlled; the specific ALDO number is to be confirmed
// from the PMC block when audio bring-up begins.
//
// Deep-sleep wakeup via the PWR button: when the chip is off, AXP2101
// holds CHIP_PU low; pressing PWR causes the AXP to release CHIP_PU,
// which brings the ESP32-C6 out of reset.
// =============================================================================

// =============================================================================
// Buttons
// =============================================================================

/// Boot button (Key1) - GPIO9, active-low (pulled up via R8 10K to VCC3V3).
/// Also the ESP32-C6 ROM bootloader strap pin: low at reset puts the
/// chip into download mode. After boot, firmware reads GPIO9 as a
/// plain input for software button-state detection.
pub const BTN_BOOT: u8 = 9;

/// PWR button (Key3) - readable via GPIO18, active HIGH.
///
/// The button bridges the AXP2101 PWRON pin to GND when pressed. PWRON
/// also drives the gate of a BSS138 N-channel MOSFET (T1) via a 1K
/// series resistor. T1's drain - pulled to VCC3V3 via R11 10K - is the
/// SYS_OUT net, which is wired to GPIO18.
///
/// Logic:
///   PWR not pressed -> PWRON high (AXP idle)  -> T1 ON  -> SYS_OUT low
///   PWR pressed     -> PWRON low (to GND)     -> T1 OFF -> SYS_OUT high
///
/// So `GPIO18 HIGH = PWR pressed`. The MOSFET is an inverting level
/// shifter, NOT a soft-power latch. Long-press shutdown (>=6 s) is
/// handled internally by the AXP2101 - no firmware GPIO ritual is
/// required to keep the rail latched on.
pub const BTN_PWR: u8 = 18;

// =============================================================================
// SYS_OUT
// =============================================================================
// The S3 board uses GPIO10 to drive a BSS138 MOSFET as a soft-power
// latch (firmware must drive it LOW at boot or the rail collapses).
// The C6 board uses the same MOSFET part but in a different role:
// PWRON drives the gate, and the inverted drain (SYS_OUT) is read by
// GPIO18 as the PWR-button state (see BTN_PWR above). There is no
// SYS_OUT *output* GPIO on the C6 and no boot-time latch ritual.

// =============================================================================
// SD card - not present on this board (C6 variant omits SD entirely).
// =============================================================================

// =============================================================================
// Audio
// ES8311 codec + ES7210 mic ADC, NS4150 speaker amplifier.
// MCLK / SCLK / LRCK are shared between codec and ADC (codec acts as
// the I2S clock master; ES7210 slaves off the same clocks and
// contributes ASDOUT into the ESP's I2S RX line).
// Constant names mirror the S3 board (SPEAKER_* for TX-side clocks,
// MIC_* for the RX-side data and amp control) even though the naming
// is a little asymmetric - keeps shared code identifier-compatible
// between boards.
// =============================================================================
pub const SPEAKER_MCLK:  u8 = 19;
pub const SPEAKER_SCLK:  u8 = 20;
pub const MIC_DSIN:      u8 = 23;  // ESP32 TX -> ES8311 DAC data in (DSDIN)
pub const MIC_ASDOUT:    u8 = 21;  // ES7210 ADC data out -> ESP32 RX (ASDOUT)
pub const MIC_LRCK:      u8 = 22;
pub const MIC_PA_CTRL:   u8 =  6;  // NS4150 amp enable (active high)

// =============================================================================
// Haptic motor - not present on this board.
// =============================================================================
