//! Public data types for the PCF85063A RTC driver.
//!
//! Kept separate from the driver impl so that types like [`DateTime`] can be
//! used in application logic (e.g. `app-core`) without pulling in the I2C
//! driver implementation.

// ---- Error type -----------------------------------------------------------------

/// Error type for RTC operations.
///
/// Generic over `E` - the I2C error type from whichever HAL is used.
#[derive(Debug)]
pub enum Error<E> {
    /// An I2C transaction failed; the inner value is the HAL's own error.
    I2c(E),
}

// ---- Date / time ----------------------------------------------------------------

/// A calendar date and time.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DateTime {
    pub year:    u16, // 2000-2099
    pub month:   u8,  // 1-12
    pub day:     u8,  // 1-31
    pub weekday: u8,  // 0=Sunday ... 6=Saturday
    pub hour:    u8,  // 0-23
    pub minute:  u8,  // 0-59
    pub second:  u8,  // 0-59
}

impl DateTime {
    pub fn new(year: u16, month: u8, day: u8, weekday: u8,
               hour: u8, minute: u8, second: u8) -> Self {
        Self { year, month, day, weekday, hour, minute, second }
    }

    /// Returns true if all fields are within legal ranges.
    ///
    /// Some PCF85063A chips don't set the OS flag on first power-up,
    /// so callers should validate the read-back value as a second check.
    pub fn is_valid(&self) -> bool {
        self.year   >= 2024 && self.year <= 2099
            && self.month   >= 1  && self.month  <= 12
            && self.day     >= 1  && self.day    <= 31
            && self.weekday <= 6
            && self.hour    <= 23
            && self.minute  <= 59
            && self.second  <= 59
    }
}

// ---- Alarm ----------------------------------------------------------------------

/// Alarm match condition.
///
/// Each field is `Option<u8>`. `Some(value)` enables matching on that field;
/// `None` disables it (the chip's AEN bit is set, meaning "don't compare").
///
/// The alarm fires when **all enabled fields** match simultaneously.
///
/// # Examples
///
/// Fire every minute at second 0:
/// ```ignore
/// Alarm { second: Some(0), ..Alarm::disabled() }
/// ```
///
/// Fire once at 07:30:00 on any day:
/// ```ignore
/// Alarm { hour: Some(7), minute: Some(30), second: Some(0), ..Alarm::disabled() }
/// ```
#[derive(Debug, Clone, Default)]
pub struct Alarm {
    /// Match on second (0-59). `None` = don't compare.
    pub second:  Option<u8>,
    /// Match on minute (0-59). `None` = don't compare.
    pub minute:  Option<u8>,
    /// Match on hour (0-23). `None` = don't compare.
    pub hour:    Option<u8>,
    /// Match on day of month (1-31). `None` = don't compare.
    pub day:     Option<u8>,
    /// Match on weekday (0=Sunday...6=Saturday). `None` = don't compare.
    pub weekday: Option<u8>,
}

impl Alarm {
    /// All fields disabled - no match will ever fire.
    pub fn disabled() -> Self {
        Self::default()
    }
}

// ---- Timer ----------------------------------------------------------------------

/// Countdown timer clock source (TCF[1:0] in Timer_mode register).
///
/// Determines the tick rate for the timer countdown value (0-255 ticks).
/// Total timeout = `value / frequency`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerClock {
    /// 4096 Hz - minimum period ~244 us, max ~62 ms.
    Hz4096 = 0b00,
    /// 64 Hz - minimum period ~15.6 ms, max ~4 s.
    Hz64   = 0b01,
    /// 1 Hz - minimum period 1 s, max 255 s.
    Hz1    = 0b10,
    /// 1/60 Hz - minimum period 60 s, max 255 min.
    Per60  = 0b11,
}

/// Timer interrupt output mode (TI_TP bit in Timer_mode register).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerOutput {
    /// INT# is held low until the timer flag (TF) is cleared. Default.
    Interrupt,
    /// INT# pulses for a short time when the timer expires.
    Pulse,
}

// ---- CLKOUT ---------------------------------------------------------------------

/// CLKOUT output frequency (COF[2:0] in Control_2 register).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClkoutFreq {
    Hz32768 = 0b000,
    Hz16384 = 0b001,
    Hz8192  = 0b010,
    Hz4096  = 0b011,
    Hz2048  = 0b100,
    Hz1024  = 0b101,
    Hz1     = 0b110,
    /// CLKOUT pin disabled (high-impedance).
    Off     = 0b111,
}

// ---- Offset calibration ---------------------------------------------------------

/// Offset calibration mode (MODE bit in Offset register).
///
/// The offset value is a 7-bit two's complement integer (`-64` to `+63`).
/// Positive values speed the clock up; negative values slow it down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OffsetMode {
    /// Correction applied once every two hours.
    /// Step size: +-4.340 ppm (fine).
    Normal,
    /// Correction applied once every minute.
    /// Step size: +-4.069 ppm (coarse, larger range).
    Coarse,
}
