//! PCF85063A RTC driver - HAL-agnostic.
//!
//! NXP PCF85063ATL on the shared I2C bus.
//! Default address: 0x51.
//! Interrupt pin: GPIO39 (active-low, driven on alarm or timer match).
//!
//! Register map (from PCF85063A datasheet Rev 7):
//!
//!   Control and status registers:
//!   0x00  Control_1     - [7]=EXT_TEST [5]=STOP [4]=SR [2]=CIE [1]=12_24 [0]=CAP_SEL
//!   0x01  Control_2     - [7]=AIE [6]=AF [5]=MI [4]=HMI [3]=TF [2:0]=COF[2:0]
//!   0x02  Offset        - [7]=MODE [6:0]=OFFSET[6:0]
//!   0x03  RAM_byte      - [7:0]=B[7:0]
//!
//!   Time and date registers:
//!   0x04  Seconds       - [7]=OS (oscillator-stop flag) [6:0]=BCD seconds
//!   0x05  Minutes       - [6:0]=BCD minutes
//!   0x06  Hours         - [5:0]=BCD hours (24h mode)
//!   0x07  Days          - [5:0]=BCD days (1-31)
//!   0x08  Weekdays      - [2:0]=weekday (0=Sunday...6=Saturday)
//!   0x09  Months        - [4:0]=BCD months (1-12)
//!   0x0A  Years         - [7:0]=BCD years (00-99, i.e. 2000-2099)
//!
//!   Alarm registers (AEN bit 7: 1 = field disabled / not compared):
//!   0x0B  Second_alarm  - [7]=AEN_S [6:0]=BCD seconds
//!   0x0C  Minute_alarm  - [7]=AEN_M [6:0]=BCD minutes
//!   0x0D  Hour_alarm    - [7]=AEN_H [5:0]=BCD hours
//!   0x0E  Day_alarm     - [7]=AEN_D [5:0]=BCD days
//!   0x0F  Weekday_alarm - [7]=AEN_W [2:0]=weekday
//!
//!   Timer registers:
//!   0x10  Timer_value   - [7:0]=T[7:0]
//!   0x11  Timer_mode    - [4:3]=TCF[1:0] [2]=TE [1]=TIE [0]=TI_TP
//!
//! Works with any I2C implementation that satisfies the `embedded-hal` traits.
//! The I2C bus is passed by mutable reference on each call so it can be shared
//! with the touch controller, PMU, and any other I2C peripheral.

pub mod types;
pub use types::*;

use embedded_hal::i2c::I2c as I2cTrait;

/// Default I2C address for the PCF85063A (fixed, not configurable).
pub const DEFAULT_ADDRESS: u8 = 0x51;

// ---- Register addresses ---------------------------------------------------------

const REG_CTRL1:        u8 = 0x00;
const REG_CTRL2:        u8 = 0x01;
const REG_OFFSET:       u8 = 0x02;
const REG_RAM:          u8 = 0x03;
const REG_SECONDS:      u8 = 0x04; // burst-read start for time: 0x04-0x0A (7 bytes)
const REG_ALARM_SECOND: u8 = 0x0B; // burst-write start for alarm: 0x0B-0x0F (5 bytes)
const REG_TIMER_VALUE:  u8 = 0x10;
const REG_TIMER_MODE:   u8 = 0x11;

// ---- Control_2 bit masks --------------------------------------------------------

const CTRL2_AIE: u8 = 1 << 7; // Alarm Interrupt Enable - drives INT# pin on alarm
const CTRL2_AF:  u8 = 1 << 6; // Alarm Flag - set by chip on match, write 0 to clear
const CTRL2_TF:  u8 = 1 << 3; // Timer Flag - set by chip on timer expiry, write 0 to clear
const CTRL2_COF: u8 = 0x07;   // COF[2:0] mask - CLKOUT frequency bits

// ---- Timer_mode bit masks -------------------------------------------------------

const TIMER_TE:    u8 = 1 << 2; // Timer Enable
const TIMER_TIE:   u8 = 1 << 1; // Timer Interrupt Enable - drives INT# pin on expiry
const TIMER_TI_TP: u8 = 1 << 0; // 0 = interrupt (INT# held low), 1 = pulse

// ---- Alarm register bit ---------------------------------------------------------

/// Set in an alarm register to disable (not compare) that field.
const AEN: u8 = 1 << 7;

// ---- Driver configuration -------------------------------------------------------

/// PCF85063A RTC driver configuration.
pub struct Config {
    /// I2C device address (default: [`DEFAULT_ADDRESS`]).
    pub address: u8,
}

impl Default for Config {
    fn default() -> Self {
        Self { address: DEFAULT_ADDRESS }
    }
}

// ---- Driver ---------------------------------------------------------------------

/// PCF85063A RTC driver.
///
/// Holds only the I2C address. The I2C bus is passed by mutable reference
/// on every call so it can be freely shared with other peripherals.
pub struct Rtc {
    addr: u8,
}

impl Rtc {
    /// Create a new RTC driver instance.
    pub fn new(config: Config) -> Self {
        Self { addr: config.address }
    }

    // ---- Initialisation ---------------------------------------------------------

    /// Initialise the PCF85063A.
    ///
    /// Performs a software reset, then configures 24h mode with the clock
    /// running and the internal 7 pF quartz capacitor selected.
    ///
    /// Returns `Ok(true)` if the oscillator-stop (OS) flag was set - meaning
    /// the clock lost power and the stored time is invalid. Call `set()` with
    /// a known time in that case.
    pub fn init<I2C, E>(&self, i2c: &mut I2C) -> Result<bool, Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        // Software reset (SR bit = 1). Resets all registers to power-on defaults.
        self.write_register(i2c, REG_CTRL1, 0x58)?;

        // Read back to verify the device is responding.
        let ctrl1 = self.read_register(i2c, REG_CTRL1)?;

        // Ensure 24h mode (12_24 bit 1 = 0), clock running (STOP bit 5 = 0),
        // and 7 pF capacitor selected (CAP_SEL bit 0 = 1). Preserve other bits.
        let ctrl1_new = (ctrl1 & !(1 << 5) & !(1 << 1)) | (1 << 0);
        if ctrl1_new != ctrl1 {
            self.write_register(i2c, REG_CTRL1, ctrl1_new)?;
        }

        // OS flag is bit 7 of the Seconds register.
        let seconds_raw = self.read_register(i2c, REG_SECONDS)?;
        Ok((seconds_raw & 0x80) != 0)
    }

    // ---- Time read / write ------------------------------------------------------

    /// Read the current date and time.
    pub fn get<I2C, E>(&self, i2c: &mut I2C) -> Result<DateTime, Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        // Burst-read 7 bytes: Seconds, Minutes, Hours, Days, Weekdays, Months, Years.
        let mut buf = [0u8; 7];
        i2c.write_read(self.addr, &[REG_SECONDS], &mut buf)
            .map_err(Error::I2c)?;

        Ok(DateTime {
            second:  bcd2bin(buf[0] & 0x7F), // mask out OS flag
            minute:  bcd2bin(buf[1] & 0x7F),
            hour:    bcd2bin(buf[2] & 0x3F),
            day:     bcd2bin(buf[3] & 0x3F),
            weekday: bcd2bin(buf[4] & 0x07),
            month:   bcd2bin(buf[5] & 0x1F),
            year:    2000 + bcd2bin(buf[6]) as u16,
        })
    }

    /// Write a new date and time. Also clears the oscillator-stop flag.
    pub fn set<I2C, E>(&self, i2c: &mut I2C, dt: &DateTime) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        // Stop the clock before writing (STOP bit = 1), then restart after.
        let ctrl1 = self.read_register(i2c, REG_CTRL1)?;
        self.write_register(i2c, REG_CTRL1, ctrl1 | (1 << 5))?;

        // Burst-write 7 time registers. Seconds bit 7 = 0 clears the OS flag.
        let buf = [
            REG_SECONDS,
            bin2bcd(dt.second),               // OS flag = 0 (cleared)
            bin2bcd(dt.minute),
            bin2bcd(dt.hour),
            bin2bcd(dt.day),
            dt.weekday & 0x07,
            bin2bcd(dt.month),
            bin2bcd((dt.year - 2000) as u8),
        ];
        i2c.write(self.addr, &buf).map_err(Error::I2c)?;

        // Restart the clock (clear STOP bit).
        self.write_register(i2c, REG_CTRL1, ctrl1 & !(1 << 5))
    }

    // ---- Alarm ------------------------------------------------------------------

    /// Program the alarm and enable the INT# interrupt pin.
    ///
    /// Only fields set to `Some(value)` are compared; `None` fields are
    /// ignored by the chip (AEN bit set). The alarm fires - and the INT#
    /// pin is driven low - when all enabled fields match simultaneously.
    ///
    /// Any previously pending alarm flag is cleared before the new alarm
    /// is armed, so the caller does not get a spurious interrupt.
    ///
    /// Call [`clear_alarm_flag`] inside the GPIO39 interrupt handler after
    /// reading the flag to re-arm the INT# pin for the next match.
    ///
    /// [`clear_alarm_flag`]: Rtc::clear_alarm_flag
    pub fn set_alarm<I2C, E>(&self, i2c: &mut I2C, alarm: &Alarm) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        // Burst-write alarm registers 0x0B-0x0F.
        // AEN=1 (bit 7) means that field is not compared.
        let buf = [
            REG_ALARM_SECOND,
            alarm.second .map_or(AEN, bin2bcd),
            alarm.minute .map_or(AEN, bin2bcd),
            alarm.hour   .map_or(AEN, bin2bcd),
            alarm.day    .map_or(AEN, bin2bcd),
            alarm.weekday.map_or(AEN, |v| v & 0x07),
        ];
        i2c.write(self.addr, &buf).map_err(Error::I2c)?;

        // Clear any stale AF flag, then enable AIE. Preserve TF and COF bits.
        let ctrl2 = self.read_register(i2c, REG_CTRL2)?;
        self.write_register(i2c, REG_CTRL2, (ctrl2 & !CTRL2_AF) | CTRL2_AIE)
    }

    /// Disable the alarm: set AEN=1 on all fields, clear AIE and AF.
    ///
    /// After this call the INT# pin will not be driven by the alarm, and
    /// any pending alarm interrupt is acknowledged.
    pub fn disable_alarm<I2C, E>(&self, i2c: &mut I2C) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let buf = [REG_ALARM_SECOND, AEN, AEN, AEN, AEN, AEN];
        i2c.write(self.addr, &buf).map_err(Error::I2c)?;

        // Clear AIE and AF, preserve TF and COF.
        let ctrl2 = self.read_register(i2c, REG_CTRL2)?;
        self.write_register(i2c, REG_CTRL2, ctrl2 & !(CTRL2_AIE | CTRL2_AF))
    }

    /// Returns `true` if the alarm flag (AF) is set.
    ///
    /// Use this for polling. In interrupt-driven code, call this inside the
    /// GPIO39 handler to confirm the interrupt source is the alarm (not the timer).
    pub fn alarm_triggered<I2C, E>(&self, i2c: &mut I2C) -> Result<bool, Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let ctrl2 = self.read_register(i2c, REG_CTRL2)?;
        Ok((ctrl2 & CTRL2_AF) != 0)
    }

    /// Clear the alarm flag (AF) to re-arm the INT# pin.
    ///
    /// Call this after handling an alarm interrupt. Does not disable the
    /// alarm - it will fire again on the next match.
    pub fn clear_alarm_flag<I2C, E>(&self, i2c: &mut I2C) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let ctrl2 = self.read_register(i2c, REG_CTRL2)?;
        self.write_register(i2c, REG_CTRL2, ctrl2 & !CTRL2_AF)
    }

    // ---- Countdown timer --------------------------------------------------------

    /// Start the countdown timer.
    ///
    /// The timer counts down from `value` (1-255) at the rate set by `clock`.
    /// When it reaches zero it sets the TF flag in Control_2 and, if `output`
    /// is [`TimerOutput::Interrupt`], drives the INT# pin (GPIO39) low until
    /// [`clear_timer_flag`] is called. With [`TimerOutput::Pulse`] the pin
    /// pulses briefly instead.
    ///
    /// The timer reloads automatically and repeats indefinitely until
    /// [`disable_timer`] is called.
    ///
    /// Any previously pending timer flag is cleared before starting.
    ///
    /// [`clear_timer_flag`]: Rtc::clear_timer_flag
    /// [`disable_timer`]: Rtc::disable_timer
    pub fn set_timer<I2C, E>(
        &self,
        i2c: &mut I2C,
        value: u8,
        clock: TimerClock,
        output: TimerOutput,
    ) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        // Write the countdown value first.
        self.write_register(i2c, REG_TIMER_VALUE, value)?;

        // Clear any stale TF flag before enabling.
        let ctrl2 = self.read_register(i2c, REG_CTRL2)?;
        self.write_register(i2c, REG_CTRL2, ctrl2 & !CTRL2_TF)?;

        // Build Timer_mode: TCF[1:0] | TE | TIE | TI_TP
        let ti_tp = match output {
            TimerOutput::Interrupt => 0,
            TimerOutput::Pulse     => TIMER_TI_TP,
        };
        let mode = ((clock as u8) << 3) | TIMER_TE | TIMER_TIE | ti_tp;
        self.write_register(i2c, REG_TIMER_MODE, mode)
    }

    /// Stop the countdown timer and clear the timer flag.
    pub fn disable_timer<I2C, E>(&self, i2c: &mut I2C) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        // Clear TE and TIE in Timer_mode (preserves TCF and TI_TP).
        let mode = self.read_register(i2c, REG_TIMER_MODE)?;
        self.write_register(i2c, REG_TIMER_MODE, mode & !(TIMER_TE | TIMER_TIE))?;

        // Clear TF in Control_2.
        let ctrl2 = self.read_register(i2c, REG_CTRL2)?;
        self.write_register(i2c, REG_CTRL2, ctrl2 & !CTRL2_TF)
    }

    /// Returns `true` if the timer flag (TF) is set.
    ///
    /// Use this for polling. In interrupt-driven code, call this inside the
    /// GPIO39 handler to confirm the interrupt source is the timer (not the alarm).
    pub fn timer_expired<I2C, E>(&self, i2c: &mut I2C) -> Result<bool, Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let ctrl2 = self.read_register(i2c, REG_CTRL2)?;
        Ok((ctrl2 & CTRL2_TF) != 0)
    }

    /// Clear the timer flag (TF) to re-arm the INT# pin.
    ///
    /// Call this after handling a timer interrupt. The timer continues
    /// running and will fire again after the next full countdown.
    pub fn clear_timer_flag<I2C, E>(&self, i2c: &mut I2C) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let ctrl2 = self.read_register(i2c, REG_CTRL2)?;
        self.write_register(i2c, REG_CTRL2, ctrl2 & !CTRL2_TF)
    }

    // ---- CLKOUT -----------------------------------------------------------------

    /// Set the CLKOUT pin frequency, or disable it.
    ///
    /// The CLKOUT pin outputs a square wave at the selected frequency.
    /// Use [`ClkoutFreq::Off`] to put the pin into high-impedance state.
    pub fn set_clkout<I2C, E>(&self, i2c: &mut I2C, freq: ClkoutFreq) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        // COF[2:0] are the low 3 bits of Control_2. Preserve all other bits.
        let ctrl2 = self.read_register(i2c, REG_CTRL2)?;
        self.write_register(i2c, REG_CTRL2, (ctrl2 & !CTRL2_COF) | (freq as u8))
    }

    // ---- Offset / calibration ---------------------------------------------------

    /// Write a clock calibration offset to trim long-term drift.
    ///
    /// `offset` is a 7-bit two's complement value (`-64` to `+63`).
    /// Positive values speed the clock up; negative values slow it down.
    ///
    /// | Mode   | Correction interval | Step size    |
    /// |--------|---------------------|--------------|
    /// | Normal | Every two hours     | +-4.340 ppm  |
    /// | Coarse | Every minute        | +-4.069 ppm  |
    ///
    /// Measure drift over several days before computing an offset - one step
    /// is small, so repeated adjustment is rarely needed.
    pub fn set_offset<I2C, E>(
        &self,
        i2c: &mut I2C,
        mode: OffsetMode,
        offset: i8,
    ) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let mode_bit: u8 = match mode {
            OffsetMode::Normal => 0,
            OffsetMode::Coarse => 1 << 7,
        };
        // Clamp to 7-bit range and mask to 7 bits.
        let offset_bits = (offset.clamp(-64, 63) as u8) & 0x7F;
        self.write_register(i2c, REG_OFFSET, mode_bit | offset_bits)
    }

    // ---- RAM byte ---------------------------------------------------------------

    /// Read the single non-volatile RAM byte.
    ///
    /// This byte survives power cycles (as long as the RTC backup supply holds).
    /// Use it for a dirty-flag, a small config value, or a boot counter.
    pub fn read_ram<I2C, E>(&self, i2c: &mut I2C) -> Result<u8, Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        self.read_register(i2c, REG_RAM)
    }

    /// Write the single non-volatile RAM byte.
    pub fn write_ram<I2C, E>(&self, i2c: &mut I2C, value: u8) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        self.write_register(i2c, REG_RAM, value)
    }

    // ---- Private helpers --------------------------------------------------------

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
        i2c.write(self.addr, &[reg, val]).map_err(Error::I2c)
    }
}

// ---- BCD helpers ----------------------------------------------------------------

fn bcd2bin(bcd: u8) -> u8 {
    (bcd >> 4) * 10 + (bcd & 0x0F)
}

fn bin2bcd(bin: u8) -> u8 {
    ((bin / 10) << 4) | (bin % 10)
}
