//! AXP2101 PMU (Power Management Unit) driver - HAL-agnostic.
//!
//! Works with any I²C implementation that satisfies the `embedded-hal` traits.
//! The firmware passes in the concrete `esp_hal` I²C type; this driver never
//! imports `esp_hal` directly.
//!
//! The I²C bus is **not** owned by this struct - it is passed by mutable
//! reference on each call. This allows the same bus to be shared with the
//! touch controller, RTC, IMU, and any other I²C peripheral.
//!
//! Rail wiring on the ESP32-S3-Touch-AMOLED-2.06 (from schematic):
//!   ALDO1 → VL1_3.3V  touch controller, IMU, RTC
//!   ALDO2 → VL2_3.3V  general 3.3 V peripherals
//!   ALDO3 → VCC3V     general 3.3 V LDO
//!   ALDO4 → VL3_1.8V  CO5300 AMOLED VDDIO
//!   BLDO1 → VL_1.2V   CO5300 AMOLED VCORE
//!   BLDO2 → VL_2.8V   CO5300 AMOLED AVDD

pub mod interrupts;
pub mod power_states;
pub mod registers;

use embedded_hal::i2c::I2c as I2cTrait;

/// Default I2C address for AXP2101 (0x34 when ADDR pin is low).
pub const DEFAULT_ADDRESS: u8 = 0x34;

/// Error type for PMU operations.
///
/// Generic over `E` - the I2C error type from whichever HAL is used.
#[derive(Debug)]
pub enum Error<E> {
    /// An I2C transaction failed; the inner value is the HAL's own error.
    I2c(E),
    /// The device did not respond with the expected chip ID.
    DeviceNotFound,
    /// A voltage value outside the 500–3500 mV range was requested.
    InvalidValue,
}

/// AXP2101 PMU driver.
///
/// Holds only the I²C address (and any future driver state). The I²C bus
/// itself is passed by mutable reference on every call so it can be freely
/// shared with other peripherals on the same bus.
pub struct Pmu {
    addr: u8,
}

/// PMU configuration.
pub struct Config {
    /// I2C device address (default: [`DEFAULT_ADDRESS`]).
    pub address: u8,
}

impl Default for Config {
    fn default() -> Self {
        Self { address: DEFAULT_ADDRESS }
    }
}

impl Pmu {
    /// Create a new PMU driver instance.
    pub fn new(config: Config) -> Self {
        Self { addr: config.address }
    }

    /// Verify the AXP2101 is present on the I²C bus and has the correct chip ID.
    ///
    /// Reads REG 03h and checks that `chip_id_h` and `chip_id_l` match the
    /// AXP2101 signature (ignoring the version bits). Returns the raw register
    /// byte on success so the caller can extract the version if needed:
    ///
    /// ```ignore
    /// let raw = pmu.check_device(&mut i2c)?;
    /// let version = (raw >> 4) & 0x03; // 0=A, 1=B
    /// ```
    pub fn check_device<I2C, E>(&self, i2c: &mut I2C) -> Result<u8, Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let raw = self.read_register(i2c, registers::CHIP_ID)?;
        if (raw & registers::CHIP_ID_MASK) != registers::CHIP_ID_VALUE {
            return Err(Error::DeviceNotFound);
        }
        Ok(raw)
    }

    /// Initialise the PMU: verify presence, set rail voltages, enable all rails.
    ///
    /// Returns the raw chip ID byte on success. The version can be extracted
    /// from bits 5:4 of that byte (0=A, 1=B).
    ///
    /// Call this early in `main`, **before** accessing the display or any other
    /// peripheral that depends on these power rails. After returning `Ok(...)`,
    /// wait at least 20 ms for the rails to stabilise.
    pub fn init<I2C, E>(&self, i2c: &mut I2C) -> Result<u8, Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        // Verify device presence and chip ID.
        let chip_id = self.check_device(i2c)?;

        // Set all output voltages while LDOs are still disabled.
        self.set_aldo1_voltage(i2c, 3300)?; // VL1_3.3V - touch, IMU, RTC
        self.set_aldo2_voltage(i2c, 3300)?; // VL2_3.3V - general 3.3 V
        self.set_aldo3_voltage(i2c, 3300)?; // VCC3V    - general 3.3 V
        self.set_aldo4_voltage(i2c, 1800)?; // VL3_1.8V - CO5300 VDDIO
        self.set_bldo1_voltage(i2c, 1200)?; // VL_1.2V  - CO5300 VCORE
        self.set_bldo2_voltage(i2c, 2800)?; // VL_2.8V  - CO5300 AVDD

        // Enable all six LDOs in one register write.
        self.enable_all_rails(i2c)?;

        Ok(chip_id)
    }

    // ---- Interrupt handling -------------------------------------------------

    /// Read all three IRQ status registers and return a combined snapshot.
    ///
    /// Call this when the IRQ pin goes low. After inspecting the result,
    /// call [`clear_interrupts`] to acknowledge and re-arm the IRQ pin.
    ///
    /// [`clear_interrupts`]: Pmu::clear_interrupts
    pub fn read_interrupts<I2C, E>(&self, i2c: &mut I2C) -> Result<interrupts::InterruptStatus, Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let r0 = self.read_register(i2c, registers::REG_IRQ_STATUS0)?;
        let r1 = self.read_register(i2c, registers::REG_IRQ_STATUS1)?;
        let r2 = self.read_register(i2c, registers::REG_IRQ_STATUS2)?;
        Ok(interrupts::InterruptStatus::new(r0, r1, r2))
    }

    /// Clear the interrupt flags that were set in `status` (write 1 to clear, RW1C).
    ///
    /// Pass the same `InterruptStatus` returned by [`read_interrupts`] so only
    /// the bits that were active get cleared - any new events that arrived in
    /// between are preserved.
    ///
    /// [`read_interrupts`]: Pmu::read_interrupts
    pub fn clear_interrupts<I2C, E>(&self, i2c: &mut I2C, status: &interrupts::InterruptStatus) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        self.write_register(i2c, registers::REG_IRQ_STATUS0, status.reg48_byte())?;
        self.write_register(i2c, registers::REG_IRQ_STATUS1, status.reg49_byte())?;
        self.write_register(i2c, registers::REG_IRQ_STATUS2, status.reg4a_byte())
    }

    /// Configure which interrupt sources drive the IRQ pin.
    ///
    /// Build an [`InterruptConfig`] using the builder pattern, then pass it here:
    ///
    /// ```ignore
    /// use drivers::pmu::interrupts::{InterruptConfig, InterruptSource};
    /// let cfg = InterruptConfig::none()
    ///     .enable(InterruptSource::VbusInsert)
    ///     .enable(InterruptSource::PowerOnShortPress);
    /// pmu.configure_interrupts(&mut i2c, &cfg)?;
    /// ```
    ///
    /// [`InterruptConfig`]: interrupts::InterruptConfig
    pub fn configure_interrupts<I2C, E>(&self, i2c: &mut I2C, cfg: &interrupts::InterruptConfig) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        self.write_register(i2c, registers::REG_IRQ_EN0, cfg.reg40_byte())?;
        self.write_register(i2c, registers::REG_IRQ_EN1, cfg.reg41_byte())?;
        self.write_register(i2c, registers::REG_IRQ_EN2, cfg.reg42_byte())
    }

    /// Set ALDO1 voltage in millivolts (valid range: 500–3500 mV, 100 mV steps).
    pub fn set_aldo1_voltage<I2C, E>(&self, i2c: &mut I2C, millivolts: u16) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        self.write_register(i2c, registers::REG_ALDO1_VOLT, Self::mv_to_register(millivolts)?)
    }

    /// Set ALDO2 voltage in millivolts.
    pub fn set_aldo2_voltage<I2C, E>(&self, i2c: &mut I2C, millivolts: u16) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        self.write_register(i2c, registers::REG_ALDO2_VOLT, Self::mv_to_register(millivolts)?)
    }

    /// Set ALDO3 voltage in millivolts.
    pub fn set_aldo3_voltage<I2C, E>(&self, i2c: &mut I2C, millivolts: u16) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        self.write_register(i2c, registers::REG_ALDO3_VOLT, Self::mv_to_register(millivolts)?)
    }

    /// Set ALDO4 voltage in millivolts.
    pub fn set_aldo4_voltage<I2C, E>(&self, i2c: &mut I2C, millivolts: u16) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        self.write_register(i2c, registers::REG_ALDO4_VOLT, Self::mv_to_register(millivolts)?)
    }

    /// Set BLDO1 voltage in millivolts.
    pub fn set_bldo1_voltage<I2C, E>(&self, i2c: &mut I2C, millivolts: u16) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        self.write_register(i2c, registers::REG_BLDO1_VOLT, Self::mv_to_register(millivolts)?)
    }

    /// Set BLDO2 voltage in millivolts.
    pub fn set_bldo2_voltage<I2C, E>(&self, i2c: &mut I2C, millivolts: u16) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        self.write_register(i2c, registers::REG_BLDO2_VOLT, Self::mv_to_register(millivolts)?)
    }

    /// Enable ALDO1–ALDO4 and BLDO1–BLDO2 (the six rails used on this board).
    ///
    /// Writes `ldo_en0::ALDO1 | ALDO2 | ALDO3 | ALDO4 | BLDO1 | BLDO2` = 0x3F
    /// to REG 90h. CPUSLDO and DLDO1/2 are left off.
    pub fn enable_all_rails<I2C, E>(&self, i2c: &mut I2C) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        use registers::ldo_en0;
        let mask = ldo_en0::ALDO1 | ldo_en0::ALDO2 | ldo_en0::ALDO3
                 | ldo_en0::ALDO4 | ldo_en0::BLDO1 | ldo_en0::BLDO2;
        self.write_register(i2c, registers::REG_LDO_EN0, mask)
    }

    /// Disable all LDOs (REG 90h and REG 91h both cleared).
    pub fn disable_all_rails<I2C, E>(&self, i2c: &mut I2C) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        self.write_register(i2c, registers::REG_LDO_EN0, 0x00)?;
        self.write_register(i2c, registers::REG_LDO_EN1, 0x00)
    }

    // ---- private helpers ----------------------------------------------------

    fn read_register<I2C, E>(&self, i2c: &mut I2C, reg: u8) -> Result<u8, Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let mut buf = [0u8; 1];
        i2c.write_read(self.addr, &[reg], &mut buf)
            .map_err(Error::I2c)?;
        Ok(buf[0])
    }

    fn write_register<I2C, E>(&self, i2c: &mut I2C, reg: u8, val: u8) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        i2c.write(self.addr, &[reg, val])
            .map_err(Error::I2c)
    }

    /// Convert millivolts to the 5-bit AXP2101 register value.
    /// Formula: `(mV − 500) / 100`. Valid range: 500–3500 mV.
    fn mv_to_register<E>(millivolts: u16) -> Result<u8, Error<E>> {
        if millivolts < 500 || millivolts > 3500 {
            return Err(Error::InvalidValue);
        }
        Ok(((millivolts - 500) / 100) as u8)
    }
}

// Re-export interrupts types so callers can do `use drivers::pmu::interrupts::*`.
pub use interrupts::{InterruptConfig, InterruptSource, InterruptStatus};
