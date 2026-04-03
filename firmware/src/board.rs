//! board.rs - pin assignments and board-level constants for the
//! Waveshare ESP32-S3-Touch-AMOLED-2.06 (32MB Flash + 8MB OPI PSRAM).
//!
//! Source: config.h from the working PlatformIO/Arduino reference project.

// =============================================================================
// Display - RM67162 AMOLED, QSPI interface
// Resolution: 410 x 502 px (portrait)
// =============================================================================
pub const LCD_SDIO0:  u8 = 4;   // QSPI Data 0 (MOSI)
pub const LCD_SDIO1:  u8 = 5;   // QSPI Data 1
pub const LCD_SDIO2:  u8 = 6;   // QSPI Data 2
pub const LCD_SDIO3:  u8 = 7;   // QSPI Data 3
pub const LCD_SCLK:   u8 = 11;  // Serial clock
pub const LCD_CS:     u8 = 12;  // Chip select (active low)
pub const LCD_RESET:  u8 = 8;   // Reset (active low)
pub const LCD_TE:     u8 = 13;  // Tear enable (optional, avoids tearing)

pub const LCD_WIDTH:  u16 = 410;
pub const LCD_HEIGHT: u16 = 502;

// Column / row offset for the RM67162 on this specific panel.
pub const LCD_COL_OFFSET: u16 = 22;
pub const LCD_ROW_OFFSET: u16 = 0;

// =============================================================================
// Shared I2C bus
// Devices on this bus: FT3168 (touch), QMI8658 (IMU), PCF85063 (RTC), AXP2101 (PMU)
// =============================================================================
pub const I2C_SDA: u8 = 15;
pub const I2C_SCL: u8 = 14;

// =============================================================================
// Touch controller - FT3168 (I2C, shared bus)
// =============================================================================
pub const TOUCH_INT:       u8 = 38;    // Interrupt (active low)
pub const TOUCH_RST:       u8 = 9;     // Reset (active low)
pub const TOUCH_I2C_ADDR:  u8 = 0x38;  // FT3168 default I2C address

// =============================================================================
// IMU - QMI8658 (I2C, shared bus)
// =============================================================================
pub const IMU_INT1: u8 = 21;    // Data-ready interrupt (INT2 only on test point TP15)

// =============================================================================
// RTC - PCF85063 (I2C, shared bus)
// =============================================================================
pub const RTC_INT: u8 = 39;     // Alarm / timer interrupt

// =============================================================================
// Power management - AXP2101 (I2C, shared bus)
// IRQ pin goes to IO expander EXIO5 (not a direct ESP32 GPIO).
// Deep sleep wakeup via power button uses CHIP_PU (ESP32 enable pin), not IRQ.
// =============================================================================

// =============================================================================
// Buttons
// =============================================================================

/// Boot button (Key1) - GPIO0, active-low (pulled up via 10K to VCC2V3).
/// Also used to enter download mode. Can be used as an ext-wakeup source
/// from ESP32 deep sleep (triggers on low level).
pub const BTN_BOOT: u8 = 0;

/// PWR button (Key3) - connected to AXP2101 PWRON pin only.
/// There is NO direct ESP32 GPIO for this button. The AXP2101 handles
/// press detection; short/long press is readable via PMU interrupt registers.
/// The system boots when the button pulls PWRON low long enough for the AXP2101
/// to assert CHIP_PU (ESP32 enable pin).

// =============================================================================
// SYS_OUT power latch
// =============================================================================

/// SYS_OUT MOSFET gate (GPIO10).
///
/// Drives the gate of BSS138LT1G N-channel MOSFET T1:
///   LOW  = FET off = SYS_OUT rail HIGH (enabled via R11 10K pull-up)
///   HIGH = FET on  = SYS_OUT rail shorted to GND (disabled)
///
/// The PWR button holds the gate LOW while the user presses it to boot.
/// Firmware must drive this LOW immediately at startup to latch the rail on.
/// Driving it HIGH (or floating it) will cut SYS_OUT and power down the system.
pub const SYS_OUT_LATCH: u8 = 10;

// =============================================================================
// SD card - SPI interface
// =============================================================================
pub const SD_MOSI: u8 = 1;
pub const SD_SCK:  u8 = 2;
pub const SD_MISO: u8 = 3;
pub const SD_CS:   u8 = 17;

// =============================================================================
// Audio
// =============================================================================
pub const SPEAKER_MCLK:  u8 = 16;
pub const SPEAKER_SCLK:  u8 = 41;
pub const MIC_DSIN:      u8 = 40;
pub const MIC_ASDOUT:    u8 = 42;
pub const MIC_LRCK:      u8 = 45;
pub const MIC_PA_CTRL:   u8 = 46;  // PA enable (active high)

// =============================================================================
// Haptic motor
// =============================================================================
pub const MOTOR_PIN: u8 = 18;
