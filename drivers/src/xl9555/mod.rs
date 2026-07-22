//! XL9555 16-bit I2C GPIO expander driver.
//!
//! Works with any I2C implementation that satisfies the `embedded-hal`
//! traits. Register map is TCA9535-compatible: two 8-bit ports with
//! input, output, polarity-inversion, and configuration registers.
//!
//! ## Pin numbering
//!
//! The datasheet names the pins `P00`-`P07` (port 0) and `P10`-`P17`
//! (port 1). This driver uses a linear index 0-15 where 0-7 map to
//! `P00`-`P07` and 8-15 map to `P10`-`P17` - the same convention as
//! the vendor Arduino libraries, so pin constants can be checked
//! against reference code without translation. Callers should keep a
//! board-level table documenting both names (see the bin's
//! `board.rs`).
//!
//! ## Power-on defaults
//!
//! All pins reset as inputs (`CONFIG = 0xFF`) with the output latch
//! high (`OUTPUT = 0xFF`). Switching a pin to output therefore drives
//! it high unless the latch is written low first - [`set_output`]
//! writes the latch before flipping the direction so a pin never
//! glitches through the wrong level.
//!
//! [`set_output`]: Xl9555::set_output

use embedded_hal::i2c::I2c as I2cTrait;

/// Default I2C address with A0/A1/A2 strapped low.
pub const DEFAULT_ADDRESS: u8 = 0x20;

mod registers {
    pub const INPUT0:  u8 = 0x00;
    pub const INPUT1:  u8 = 0x01;
    pub const OUTPUT0: u8 = 0x02;
    pub const OUTPUT1: u8 = 0x03;
    // Polarity inversion registers 0x04/0x05 exist but are not used;
    // leaving them at their reset value (no inversion) is always what
    // we want.
    pub const CONFIG0: u8 = 0x06;
    pub const CONFIG1: u8 = 0x07;
}

/// Driver error.
#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error<E> {
    /// An I2C transaction failed; the inner value is the HAL's own error.
    I2c(E),
    /// A pin index outside 0-15 was requested.
    InvalidPin,
}

/// XL9555 expander driver.
///
/// Holds only the I2C address. The I2C bus itself is passed by mutable
/// reference on every call so it can be freely shared with other
/// peripherals on the same bus.
pub struct Xl9555 {
    addr: u8,
}

/// Expander configuration.
pub struct Config {
    /// I2C device address (default: [`DEFAULT_ADDRESS`]).
    pub address: u8,
}

impl Default for Config {
    fn default() -> Self {
        Self { address: DEFAULT_ADDRESS }
    }
}

impl Xl9555 {
    /// Create a new expander driver instance.
    pub fn new(config: Config) -> Self {
        Self { addr: config.address }
    }

    /// Verify the expander responds on the bus.
    ///
    /// The XL9555 has no chip-ID register, so presence is checked by
    /// reading the port-0 input register. Returns the raw input byte
    /// on success.
    pub fn probe<I2C, E>(&self, i2c: &mut I2C) -> Result<u8, Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        self.read_register(i2c, registers::INPUT0)
    }

    /// Configure `pin` as an output driving `high`.
    ///
    /// Writes the output latch first, then clears the pin's
    /// configuration bit (0 = output), so the pin transitions straight
    /// from Hi-Z to the requested level without glitching through the
    /// power-on-high latch value.
    pub fn set_output<I2C, E>(&self, i2c: &mut I2C, pin: u8, high: bool) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let (out_reg, cfg_reg, bit) = Self::pin_regs(pin)?;
        self.update_register(i2c, out_reg, bit, high)?;
        self.update_register(i2c, cfg_reg, bit, false)
    }

    /// Drive an already-configured output pin to `high`.
    ///
    /// Only touches the output latch; direction is left alone. Use
    /// [`set_output`](Xl9555::set_output) the first time a pin is
    /// driven.
    pub fn write_pin<I2C, E>(&self, i2c: &mut I2C, pin: u8, high: bool) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let (out_reg, _, bit) = Self::pin_regs(pin)?;
        self.update_register(i2c, out_reg, bit, high)
    }

    /// Configure `pin` as an input (the power-on default direction).
    pub fn set_input<I2C, E>(&self, i2c: &mut I2C, pin: u8) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let (_, cfg_reg, bit) = Self::pin_regs(pin)?;
        self.update_register(i2c, cfg_reg, bit, true)
    }

    /// Read the current level of `pin` from the input register.
    ///
    /// Valid for pins in either direction - the input register always
    /// reflects the physical pin state.
    pub fn read_pin<I2C, E>(&self, i2c: &mut I2C, pin: u8) -> Result<bool, Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let (out_reg, _, bit) = Self::pin_regs(pin)?;
        // INPUTx sits two registers below OUTPUTx in the map.
        let in_reg = out_reg - 2;
        let val = self.read_register(i2c, in_reg)?;
        Ok(val & bit != 0)
    }

    /// Read both input ports as one 16-bit word (bit 0 = P00, bit 15 = P17).
    pub fn read_all<I2C, E>(&self, i2c: &mut I2C) -> Result<u16, Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let lo = self.read_register(i2c, registers::INPUT0)?;
        let hi = self.read_register(i2c, registers::INPUT1)?;
        Ok(u16::from_le_bytes([lo, hi]))
    }

    // ---- Register access ----------------------------------------------------

    /// Map a linear pin index to its (output register, config register,
    /// bit mask) triple.
    fn pin_regs<E>(pin: u8) -> Result<(u8, u8, u8), Error<E>> {
        match pin {
            0..=7 => Ok((registers::OUTPUT0, registers::CONFIG0, 1 << pin)),
            8..=15 => Ok((registers::OUTPUT1, registers::CONFIG1, 1 << (pin - 8))),
            _ => Err(Error::InvalidPin),
        }
    }

    /// Read-modify-write a single bit in `register`.
    fn update_register<I2C, E>(
        &self,
        i2c: &mut I2C,
        register: u8,
        bit: u8,
        set: bool,
    ) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let cur = self.read_register(i2c, register)?;
        let new = if set { cur | bit } else { cur & !bit };
        if new != cur {
            self.write_register(i2c, register, new)?;
        }
        Ok(())
    }

    fn read_register<I2C, E>(&self, i2c: &mut I2C, register: u8) -> Result<u8, Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let mut buf = [0u8; 1];
        i2c.write_read(self.addr, &[register], &mut buf)
            .map_err(Error::I2c)?;
        Ok(buf[0])
    }

    fn write_register<I2C, E>(&self, i2c: &mut I2C, register: u8, value: u8) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        i2c.write(self.addr, &[register, value]).map_err(Error::I2c)
    }
}
