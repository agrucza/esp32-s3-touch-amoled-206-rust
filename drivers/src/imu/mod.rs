//! QMI8658C 6-axis IMU driver - HAL-agnostic.
//!
//! Works with any I²C implementation that satisfies the `embedded-hal` traits.
//! The firmware passes in the concrete `esp_hal` I²C type; this driver never
//! imports `esp_hal` directly.
//!
//! The I²C bus is **not** owned by this struct - it is passed by mutable
//! reference on each call. This allows the same bus to be shared with the
//! PMU, touch controller, RTC, and any other I²C peripheral.
//!
//! # Wiring on ESP32-S3-Touch-AMOLED-2.06
//!
//! | Signal   | GPIO |
//! |----------|------|
//! | I2C_SDA  | 15   |
//! | I2C_SCL  | 14   |
//! | INT1     | 21   |
//!
//! The SDO/SA0 pin is tied to GND, giving I²C address 0x6B.
//! The CS pin is tied to VCC3V3, selecting I²C mode (not SPI).
//!
//! # Startup timing
//!
//! The QMI8658C requires **150 ms** from when VDD/VDDIO are within 1% of their
//! final values before any register may be written. The PMU enables the 3.3 V
//! rail early in `main`; ensure at least 150 ms has elapsed before calling
//! [`Qmi8658::init`]. In practice the display initialisation sequence
//! (~300 ms of delays) runs in between, so no extra wait is needed.

pub mod registers;
pub mod types;

pub use types::*;

use embedded_hal::i2c::I2c as I2cTrait;

/// Default I²C address when SDO/SA0 is tied to GND.
pub const ADDR: u8 = 0x6B;

/// QMI8658C IMU driver.
///
/// Holds the I²C address, configured scale factors, and an optional software
/// gyroscope bias. Call [`set_gyro_bias`] after [`collect_gyro_bias`] to
/// activate it; [`read`] then subtracts the stored values from every sample.
///
/// [`set_gyro_bias`]: Qmi8658::set_gyro_bias
/// [`collect_gyro_bias`]: Qmi8658::collect_gyro_bias
/// [`read`]: Qmi8658::read
pub struct Qmi8658 {
    addr:        u8,
    accel_scale: AccelScale,
    gyro_scale:  GyroScale,
    /// Subtracted from raw gyro readings in `read()`. Zero until `set_gyro_bias`.
    gyro_bias:   (i16, i16, i16),
}

/// Driver configuration.
pub struct Config {
    /// I²C device address (default: [`ADDR`]).
    pub address:     u8,
    /// Accelerometer full-scale range (default: ±8 g).
    pub accel_scale: AccelScale,
    /// Gyroscope full-scale range (default: ±256 dps).
    pub gyro_scale:  GyroScale,
    /// Accelerometer output data rate (default: 125 Hz).
    pub accel_odr:   Odr,
    /// Gyroscope output data rate (default: 125 Hz).
    pub gyro_odr:    Odr,
    /// Gyroscope low-pass filter (default: Mode1 = 3.59% of ODR).
    /// `None` disables the filter.
    pub gyro_lpf:    Option<LpfMode>,
    /// Accelerometer low-pass filter (default: disabled).
    pub accel_lpf:   Option<LpfMode>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            address:     ADDR,
            accel_scale: AccelScale::G8,
            gyro_scale:  GyroScale::Dps256,
            accel_odr:   Odr::Hz125,
            gyro_odr:    Odr::Hz125,
            gyro_lpf:    Some(LpfMode::Mode1),
            accel_lpf:   None,
        }
    }
}

impl Qmi8658 {
    /// Create a new driver instance. Call [`init`] before reading data.
    ///
    /// [`init`]: Qmi8658::init
    pub fn new(config: Config) -> Self {
        Self {
            addr:        config.address,
            accel_scale: config.accel_scale,
            gyro_scale:  config.gyro_scale,
            gyro_bias:   (0, 0, 0),
        }
    }

    /// Initialise the QMI8658C.
    ///
    /// 1. Verifies the WHO_AM_I register returns the expected chip ID (0x05).
    /// 2. Enables address auto-increment in CTRL1 for burst reads.
    /// 3. Configures the accelerometer and gyroscope ODR and full-scale range.
    /// 4. Enables both sensors via CTRL7.
    ///
    /// Returns `Err(Error::DeviceNotFound)` if the chip does not respond with
    /// the correct ID.
    pub fn init<I2C, E>(&self, i2c: &mut I2C, config: &Config) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        // Verify device identity.
        let chip_id = self.read_register(i2c, registers::WHO_AM_I)?;
        if chip_id != registers::CHIP_ID {
            return Err(Error::DeviceNotFound);
        }

        // CTRL1: keep default SPI_BE bit, add AUTO_INCREMENT for burst reads.
        // Default 0x20 | 0x40 = 0x60.
        self.write_register(i2c, registers::CTRL1,
            registers::ctrl1::SPI_BIG_ENDIAN | registers::ctrl1::AUTO_INCREMENT)?;

        // CTRL2: accelerometer full-scale and ODR (bit 7 = 0 = self-test off).
        let ctrl2 = (config.accel_scale.ctrl2_bits() << registers::ctrl2::FS_SHIFT)
                  | config.accel_odr.bits();
        self.write_register(i2c, registers::CTRL2, ctrl2)?;

        // CTRL3: gyroscope full-scale and ODR (bit 7 = 0 = self-test off).
        let ctrl3 = (config.gyro_scale.ctrl3_bits() << registers::ctrl3::FS_SHIFT)
                  | config.gyro_odr.bits();
        self.write_register(i2c, registers::CTRL3, ctrl3)?;

        // CTRL5: low-pass filter configuration for gyroscope and accelerometer.
        let ctrl5 = {
            let g = match config.gyro_lpf {
                Some(m) => (m.bits() << registers::ctrl5::GLPF_MODE_SHIFT) | registers::ctrl5::GLPF_EN,
                None    => 0,
            };
            let a = match config.accel_lpf {
                Some(m) => (m.bits() << registers::ctrl5::ALPF_MODE_SHIFT) | registers::ctrl5::ALPF_EN,
                None    => 0,
            };
            g | a
        };
        self.write_register(i2c, registers::CTRL5, ctrl5)?;

        // CTRL7: enable both accelerometer and gyroscope.
        self.write_register(i2c, registers::CTRL7,
            registers::ctrl7::ACCEL_EN | registers::ctrl7::GYRO_EN)?;

        Ok(())
    }

    /// Read a snapshot of all sensor data in a single 14-byte burst.
    ///
    /// Reads TEMP_L through GZ_H (registers 0x33-0x40) in one I²C transaction.
    /// This guarantees that temperature, accelerometer, and gyroscope readings
    /// all come from the same sample.
    ///
    /// The raw i16 values can be converted to physical units using the scale
    /// factors stored in the [`Config`] passed to [`init`]:
    ///
    /// ```ignore
    /// let ax_g   = data.accel_x as f32 / AccelScale::G8.lsb_per_g() as f32;
    /// let gx_dps = data.gyro_x  as f32 / GyroScale::Dps256.lsb_per_dps() as f32;
    /// let temp_c = data.temp_celsius(); // integer degrees
    /// ```
    pub fn read<I2C, E>(&self, i2c: &mut I2C) -> Result<ImuData, Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        // Burst-read 14 bytes starting at TEMP_L (0x33):
        // [0..1] = TEMP, [2..7] = ACCEL XYZ, [8..13] = GYRO XYZ
        let mut buf = [0u8; 14];
        i2c.write_read(self.addr, &[registers::TEMP_L], &mut buf)
            .map_err(Error::I2c)?;

        Ok(ImuData {
            temp_raw: i16::from_le_bytes([buf[0],  buf[1]]),
            accel_x:  i16::from_le_bytes([buf[2],  buf[3]]),
            accel_y:  i16::from_le_bytes([buf[4],  buf[5]]),
            accel_z:  i16::from_le_bytes([buf[6],  buf[7]]),
            gyro_x:   i16::from_le_bytes([buf[8],  buf[9]]).saturating_sub(self.gyro_bias.0),
            gyro_y:   i16::from_le_bytes([buf[10], buf[11]]).saturating_sub(self.gyro_bias.1),
            gyro_z:   i16::from_le_bytes([buf[12], buf[13]]).saturating_sub(self.gyro_bias.2),
        })
    }

    /// Check whether new accelerometer and/or gyroscope data is available.
    ///
    /// Reads STATUS0 (0x2E) and returns the raw byte. Test individual flags
    /// with the masks in [`registers::status0`]:
    ///
    /// ```ignore
    /// use drivers::imu::registers::status0;
    /// let s = imu.status(&mut i2c)?;
    /// if s & status0::ACCEL_READY != 0 { /* new accel sample */ }
    /// if s & status0::GYRO_READY  != 0 { /* new gyro sample */  }
    /// ```
    pub fn status<I2C, E>(&self, i2c: &mut I2C) -> Result<u8, Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        self.read_register(i2c, registers::STATUS0)
    }

    /// Read the WHO_AM_I and REVISION_ID registers.
    ///
    /// Returns `(chip_id, revision_id)`. Expected: `(0x05, 0x79)`.
    pub fn read_ids<I2C, E>(&self, i2c: &mut I2C) -> Result<(u8, u8), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let chip_id  = self.read_register(i2c, registers::WHO_AM_I)?;
        let revision = self.read_register(i2c, registers::REVISION_ID)?;
        Ok((chip_id, revision))
    }

    /// Collect a gyroscope bias estimate by averaging `n` distinct hardware samples.
    ///
    /// The board must be **completely still** during this call. Before reading
    /// each sample the method polls STATUS0 until the gyroscope data-ready flag
    /// (bit 1) is set, ensuring every one of the `n` samples is a genuinely new
    /// measurement from the hardware. At 125 Hz ODR this takes `n × 8 ms`
    /// (~512 ms for n=64).
    ///
    /// Returns `Err(Error::Timeout)` if a fresh sample does not arrive within
    /// 1000 I²C poll attempts (~100 ms) for any given sample slot.
    ///
    /// The returned `(bias_x, bias_y, bias_z)` values can be passed directly
    /// to [`set_gyro_bias`].
    ///
    /// [`set_gyro_bias`]: Qmi8658::set_gyro_bias
    pub fn collect_gyro_bias<I2C, E>(&self, i2c: &mut I2C, n: u8) -> Result<(i16, i16, i16), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        let mut sum_x: i32 = 0;
        let mut sum_y: i32 = 0;
        let mut sum_z: i32 = 0;

        for _ in 0..n {
            // Wait for a fresh gyroscope sample (STATUS0 bit 1 = gDA).
            let mut ready = false;
            for _ in 0..1000u16 {
                let s = self.read_register(i2c, registers::STATUS0)?;
                if s & registers::status0::GYRO_READY != 0 {
                    ready = true;
                    break;
                }
            }
            if !ready {
                return Err(Error::Timeout);
            }

            // Burst-read 6 bytes starting at GX_L (0x3B).
            let mut buf = [0u8; 6];
            i2c.write_read(self.addr, &[registers::GX_L], &mut buf)
                .map_err(Error::I2c)?;
            sum_x += i16::from_le_bytes([buf[0], buf[1]]) as i32;
            sum_y += i16::from_le_bytes([buf[2], buf[3]]) as i32;
            sum_z += i16::from_le_bytes([buf[4], buf[5]]) as i32;
        }

        let n = n as i32;
        Ok(((sum_x / n) as i16, (sum_y / n) as i16, (sum_z / n) as i16))
    }

    /// Store gyroscope bias values for software subtraction in [`read`].
    ///
    /// Pass the values returned by [`collect_gyro_bias`]. Every subsequent
    /// call to `read()` subtracts these raw i16 values from the gyro output
    /// before returning, so the readings sit near zero when the device is still.
    ///
    /// [`read`]: Qmi8658::read
    /// [`collect_gyro_bias`]: Qmi8658::collect_gyro_bias
    pub fn set_gyro_bias(&mut self, bias_x: i16, bias_y: i16, bias_z: i16) {
        self.gyro_bias = (bias_x, bias_y, bias_z);
    }

    /// Apply gyroscope bias offsets to the raw output registers using the
    /// `CTRL_CMD_GYRO_HOST_DELTA_OFFSET` CTRL9 command (code 0x0A).
    ///
    /// Pass the raw i16 bias values returned by [`collect_gyro_bias`]. This
    /// method converts them to the 11.5 fixed-point format the command requires
    /// (using the gyro scale stored at construction time), writes them into
    /// CAL1-CAL3, issues the command, and polls STATUSINT bit 7 for completion.
    ///
    /// After this call the chip subtracts the offsets internally before
    /// placing data in the GX/GY/GZ output registers - no software correction
    /// is needed in [`read`].
    ///
    /// The calibration is **volatile**: it is lost on power-off or reset.
    /// Call once at every boot (re-collect with [`collect_gyro_bias`], or load
    /// saved values from flash once persistent storage is implemented).
    ///
    /// [`collect_gyro_bias`]: Qmi8658::collect_gyro_bias
    /// [`read`]: Qmi8658::read
    pub fn calibrate_gyro<I2C, E>(&self, i2c: &mut I2C, bias_x: i16, bias_y: i16, bias_z: i16) -> Result<(), Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        // Convert raw sensor units to 11.5 fixed-point (5 fractional bits).
        // Formula: delta = bias_raw * 32 / lsb_per_dps
        // Example at ±256 dps (128 LSB/dps): bias 369 → 369*32/128 = 92
        let scale = self.gyro_scale.lsb_per_dps() as i32;
        let dx = ((bias_x as i32 * 32) / scale) as i16;
        let dy = ((bias_y as i32 * 32) / scale) as i16;
        let dz = ((bias_z as i32 * 32) / scale) as i16;

        let [dx_l, dx_h] = dx.to_le_bytes();
        let [dy_l, dy_h] = dy.to_le_bytes();
        let [dz_l, dz_h] = dz.to_le_bytes();

        self.write_register(i2c, registers::CAL1_L, dx_l)?;
        self.write_register(i2c, registers::CAL1_H, dx_h)?;
        self.write_register(i2c, registers::CAL2_L, dy_l)?;
        self.write_register(i2c, registers::CAL2_H, dy_h)?;
        self.write_register(i2c, registers::CAL3_L, dz_l)?;
        self.write_register(i2c, registers::CAL3_H, dz_h)?;

        // Issue CTRL_CMD_GYRO_HOST_DELTA_OFFSET - applies offsets to the raw
        // GX/GY/GZ output registers (unlike GYRO_BIAS 0x01 which only affects
        // the AttitudeEngine output).
        self.write_register(i2c, registers::CTRL9, registers::cmd::GYRO_HOST_DELTA_OFFSET)?;

        // Poll STATUSINT (0x2D) bit 7 (CmdDone).
        // The Rev 0.6 datasheet marks this bit as Reserved, but actual silicon
        // (rev 0x7C+) uses bit 7 of STATUSINT - confirmed by reading 0x81 after
        // the command while STATUS1 stayed 0x00.
        // At 400 kHz I2C each read takes ~100 µs; 1000 retries = ~100 ms max.
        let mut done = false;
        for _ in 0..1000u16 {
            let s = self.read_register(i2c, registers::STATUSINT)?;
            if s & registers::statusint::CMD_DONE != 0 {
                done = true;
                break;
            }
        }
        if !done {
            return Err(Error::Timeout);
        }

        // Acknowledge: write NOP so the device clears CmdDone.
        self.write_register(i2c, registers::CTRL9, registers::cmd::NOP)
    }

    /// Returns the accelerometer scale this driver was configured with.
    pub fn accel_scale(&self) -> AccelScale { self.accel_scale }

    /// Returns the gyroscope scale this driver was configured with.
    pub fn gyro_scale(&self)  -> GyroScale  { self.gyro_scale  }

    // ---- diagnostic helpers -----------------------------------------------------

    /// Read any single register by address. Useful for debugging CTRL9 failures.
    pub fn read_raw_register<I2C, E>(&self, i2c: &mut I2C, reg: u8) -> Result<u8, Error<E>>
    where
        I2C: I2cTrait<Error = E>,
    {
        self.read_register(i2c, reg)
    }

    // ---- private helpers --------------------------------------------------------

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
}
