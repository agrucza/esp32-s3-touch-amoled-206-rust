//! FT3168 I²C capacitive touch controller driver - HAL-agnostic.
//!
//! Works with any I²C implementation that satisfies the `embedded-hal` traits.
//! The I²C bus is passed by mutable reference on each call so it can be shared
//! with other peripherals (PMU, RTC, IMU).
//!
//! The RST pin is owned by the driver because it belongs exclusively to this
//! device and is only used during initialisation.
//!
//! Register map (FocalTech FT3x family):
//!   0x02  TD_STATUS  - touch point count [3:0]
//!   0x03  P1_XH      - event[7:6], X[11:8][3:0]
//!   0x04  P1_XL      - X[7:0]
//!   0x05  P1_YH      - touch ID[7:4], Y[11:8][3:0]
//!   0x06  P1_YL      - Y[7:0]
//!   0xA3  CHIP_ID    - 0x54 for FT3168
//!   0xA5  POWER_MODE - Active/Monitor/Standby/Hibernate selector
//!   0xA6  FW_VER     - firmware version

use embedded_hal::digital::OutputPin;
use embedded_hal::i2c::I2c as I2cTrait;

/// Default I²C address for FT3168.
pub const ADDR: u8 = 0x38;

const REG_TD_STATUS: u8 = 0x02;
const REG_P1_XH:     u8 = 0x03;
const REG_CHIP_ID:   u8 = 0xA3;
/// Power-mode register - write [`PowerMode`] here. Pub so callers
/// with raw I²C bus access (e.g. a sync-from-sleep handler that can't
/// dispatch to the touch task) can flip the chip directly.
pub const REG_POWER_MODE: u8 = 0xA5;
const REG_FW_VER:    u8 = 0xA6;

/// Power / operating mode of the FT3168. Written to
/// [`REG_POWER_MODE`] (0xA5) via [`FT3168::set_power_mode`].
///
/// Per the FT3168 datasheet section 2.2 (which describes the
/// modes at a high level but does not document the register) and
/// FocalTech's own reference driver for the FT3x68 family, which
/// Waveshare ships for this board and from which these register
/// values were lifted:
///
/// * [`Active`] - full scan at the configured rate (default
///   60 fps). Reports coordinates. Typical current draw 1.5 mA
///   per the datasheet's DC characteristics table.
/// * [`Monitor`] - lower programmable scan rate; the chip only
///   looks for "is there a touch?" and does not calculate
///   coordinates. On touch detection it auto-switches back to
///   Active and the host sees a normal touch event. Typical
///   current draw 30 µA. Ideal for wake-on-touch.
/// * [`Standby`] - chip not scanning. Intermediate between
///   Monitor and Hibernate. Not described in the public datasheet
///   but present in the FocalTech reference driver.
/// * [`Hibernate`] - deepest sleep. Analog circuits off, MCU
///   stopped. Only `RESETB` or a host-driven wake signal can
///   bring the chip back. Typical current draw 10 µA.
///
/// Register values come from the reference driver at
/// `Arduino_DriveBus/src/touch_chip/Arduino_FT3x68.cpp`
/// (Waveshare ESP32-S3-Touch-AMOLED-2.06 BSP).
///
/// [`Active`]: PowerMode::Active
/// [`Monitor`]: PowerMode::Monitor
/// [`Standby`]: PowerMode::Standby
/// [`Hibernate`]: PowerMode::Hibernate
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerMode {
    Active    = 0x00,
    Monitor   = 0x01,
    Standby   = 0x02,
    Hibernate = 0x03,
}

/// Touch event returned by [`FT3168::read`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TouchEvent {
    /// Finger down or dragging - display coordinates (x, y).
    Pressed { x: u16, y: u16 },
    /// All fingers lifted (first read with count=0 after a press).
    Released,
    /// No touch and no recent lift - nothing to report.
    None,
}

/// FT3168 driver.
///
/// Owns the RST output pin. The I²C bus is passed by mutable reference on
/// every call so it can be freely shared with other peripherals on the same bus.
pub struct FT3168<RST> {
    reset:       RST,
    was_pressed: bool,
}

impl<RST: OutputPin> FT3168<RST> {
    /// Create a new driver instance. `reset` is the RST output pin.
    pub fn new(reset: RST) -> Self {
        Self { reset, was_pressed: false }
    }

    /// Drive RST high (inactive).
    pub fn reset_high(&mut self) { self.reset.set_high().ok(); }

    /// Drive RST low (active - resets the controller).
    pub fn reset_low(&mut self) { self.reset.set_low().ok(); }

    /// Verify device presence and read chip / firmware IDs.
    ///
    /// Call after the hardware reset sequence has completed. Returns `Ok(())`
    /// if the device responds; returns `Err(())` if the I²C transaction fails.
    pub fn init<I2C, E>(&mut self, i2c: &mut I2C) -> Result<(), ()>
    where
        I2C: I2cTrait<Error = E>,
    {
        let chip_id = rd(i2c, REG_CHIP_ID)?;
        let fw_ver  = rd(i2c, REG_FW_VER)?;
        let _ = (chip_id, fw_ver); // available to the caller via return if needed
        Ok(())
    }

    /// Read chip ID register (0xA3) and firmware version (0xA6).
    ///
    /// Returns `(chip_id, fw_ver)` or `Err(())` on I²C failure.
    pub fn read_ids<I2C, E>(&self, i2c: &mut I2C) -> Result<(u8, u8), ()>
    where
        I2C: I2cTrait<Error = E>,
    {
        let chip_id = rd(i2c, REG_CHIP_ID)?;
        let fw_ver  = rd(i2c, REG_FW_VER)?;
        Ok((chip_id, fw_ver))
    }

    /// Returns `true` if the last [`read`] saw a finger on screen.
    ///
    /// Use this to keep polling even after the INT pin goes high, to ensure
    /// the lift-off (`Released`) event is never missed.
    ///
    /// [`read`]: FT3168::read
    pub fn is_pressed(&self) -> bool {
        self.was_pressed
    }

    /// Write the power / operating mode register (0xA5).
    ///
    /// See [`PowerMode`] for the available modes and their power /
    /// wake characteristics. The caller is responsible for any
    /// follow-up sequencing: Monitor mode auto-wakes on touch and
    /// needs nothing extra, but Standby / Hibernate only come back
    /// on RESETB or a host-side wake signal, and after a Hibernate
    /// exit the chip needs to be re-initialised as at boot.
    ///
    /// Returns `Err(())` on I²C failure.
    pub fn set_power_mode<I2C, E>(&self, i2c: &mut I2C, mode: PowerMode) -> Result<(), ()>
    where
        I2C: I2cTrait<Error = E>,
    {
        i2c.write(ADDR, &[REG_POWER_MODE, mode as u8]).map_err(|_| ())
    }

    /// Read the current touch state.
    ///
    /// Call when INT is low, or poll freely. Returns:
    /// - `Pressed { x, y }` - finger is on screen (raw panel coordinates)
    /// - `Released`          - finger just lifted (first read after a press with count=0)
    /// - `None`              - no touch, no change
    pub fn read<I2C, E>(&mut self, i2c: &mut I2C) -> TouchEvent
    where
        I2C: I2cTrait<Error = E>,
    {
        let count = match rd(i2c, REG_TD_STATUS) {
            Ok(v)  => v & 0x0F,
            Err(_) => return TouchEvent::None,
        };

        if count == 0 {
            if self.was_pressed {
                self.was_pressed = false;
                return TouchEvent::Released;
            }
            return TouchEvent::None;
        }

        // Burst-read P1_XH, P1_XL, P1_YH, P1_YL in one transaction.
        let mut buf = [0u8; 4];
        if i2c.write_read(ADDR, &[REG_P1_XH], &mut buf).is_err() {
            return TouchEvent::None;
        }

        let x = (((buf[0] & 0x0F) as u16) << 8) | (buf[1] as u16);
        let y = (((buf[2] & 0x0F) as u16) << 8) | (buf[3] as u16);

        self.was_pressed = true;
        TouchEvent::Pressed { x, y }
    }
}

// ---- private helper --------------------------------------------------------

fn rd<I2C, E>(i2c: &mut I2C, reg: u8) -> Result<u8, ()>
where
    I2C: I2cTrait<Error = E>,
{
    let mut buf = [0u8; 1];
    i2c.write_read(ADDR, &[reg], &mut buf).map_err(|_| ())?;
    Ok(buf[0])
}
