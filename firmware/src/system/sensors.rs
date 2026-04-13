use crate::events::SystemEvent;
use drivers::imu::{ImuData, Qmi8658, Config as ImuConfig, Odr, WomConfig, WomInterrupt};
use drivers::rtc::{Rtc, Config as RtcConfig, DateTime as RtcDateTime};
use embedded_hal::i2c::I2c;
use embassy_time::{Duration, Timer};

/// Snapshot of all sensor readings at one point in time. Returned
/// by [`SensorSystem::snapshot`] so the caller doesn't have to
/// individually pull each reading.
#[derive(Default)]
pub struct SensorSnapshot {
    /// Current wall-clock time from the RTC.
    pub time: Option<RtcDateTime>,
    /// IMU accel + gyro + temperature reading.
    pub imu: Option<ImuData>,
}

pub struct SensorSystem {
    pub imu: Qmi8658,
    pub rtc: Rtc,
    last_minute: u8,
}

impl SensorSystem {
    /// Initialize RTC and IMU. Includes ~500ms gyro bias calibration.
    pub async fn init(i2c: &mut impl I2c) -> Self {
        // --- RTC ---
        log::info!("RTC: initializing PCF85063...");
        let rtc = Rtc::new(RtcConfig::default());
        match rtc.init(i2c) {
            Err(_) => log::error!("RTC: device not found on I2C bus"),
            Ok(os_flag) => {
                if os_flag {
                    log::warn!("RTC: oscillator-stop flag set - time is invalid");
                } else {
                    log::info!("RTC: oscillator running, time is valid");
                }

                let needs_set = os_flag || match rtc.get(i2c) {
                    Ok(ref dt) => !dt.is_valid(),
                    Err(_) => true,
                };

                if needs_set {
                    log::warn!("RTC: time invalid - setting default");
                    let default_time = RtcDateTime::new(2026, 3, 30, 0, 12, 0, 0);
                    if rtc.set(i2c, &default_time).is_err() {
                        log::error!("RTC: failed to set time");
                    }
                }

                match rtc.get(i2c) {
                    Ok(dt) => log::info!("RTC: {:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                        dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second),
                    Err(_) => log::error!("RTC: failed to read time"),
                }
            }
        }

        // --- IMU ---
        log::info!("IMU: initializing QMI8658C...");
        let imu_config = ImuConfig::default();
        let mut imu = Qmi8658::new(ImuConfig::default());

        // Soft-reset to clear any leftover state (WoM, pedometer, etc.)
        // from a previous boot that didn't power-cycle the chip.
        if imu.soft_reset(i2c).is_ok() {
            log::info!("IMU: soft reset OK");
            // Wait for the chip to come back up after reset.
            Timer::after(Duration::from_millis(20)).await;
        } else {
            log::warn!("IMU: soft reset failed (continuing anyway)");
        }

        match imu.init(i2c, &imu_config) {
            Err(_) => log::error!("IMU: device not found at I2C address 0x{:02X}", drivers::imu::ADDR),
            Ok(()) => {
                match imu.read_ids(i2c) {
                    Ok((chip_id, rev)) => log::info!("IMU: QMI8658C chip_id=0x{:02X} rev=0x{:02X}", chip_id, rev),
                    Err(_) => log::warn!("IMU: init OK but failed to read IDs"),
                }

                Timer::after(Duration::from_millis(100)).await;

                log::info!("IMU: collecting gyro bias (keep device still ~512ms)...");
                match imu.collect_gyro_bias(i2c, 64) {
                    Err(_) => log::error!("IMU: failed to collect gyro bias"),
                    Ok((bx, by, bz)) => {
                        log::info!("IMU: gyro bias raw [{} {} {}]", bx, by, bz);
                        imu.set_gyro_bias(bx, by, bz);
                        log::info!("IMU: gyro bias applied (software)");
                    }
                }
            }
        }

        Self { imu, rtc, last_minute: 0xFF }
    }

    /// Poll for time changes, push MinuteChanged when the minute rolls over.
    pub fn poll(&mut self, i2c: &mut impl I2c, events: &mut heapless::Vec<SystemEvent, 8>) {
        if let Ok(dt) = self.rtc.get(i2c) {
            if dt.minute != self.last_minute {
                self.last_minute = dt.minute;
                let _ = events.push(SystemEvent::MinuteChanged);
            }
        }
    }

    /// Read current time from RTC.
    pub fn read_time(&self, i2c: &mut impl I2c) -> Option<RtcDateTime> {
        self.rtc.get(i2c).ok()
    }

    /// Collect all sensor readings into a single snapshot.
    pub fn snapshot(&mut self, i2c: &mut impl I2c) -> SensorSnapshot {
        SensorSnapshot {
            time: self.rtc.get(i2c).ok(),
            imu: self.imu.read(i2c).ok(),
        }
    }

    /// Enter Wake-on-Motion mode per QMI8658C datasheet section 9.4:
    ///
    /// 1. Disable sensors (CTRL7 = 0x00)
    /// 2. Switch accelerometer ODR to 31.25 Hz (low power, less
    ///    sensitive to micro-vibrations - real motion still
    ///    detected with plenty of margin)
    /// 3. Write WoM threshold, interrupt pin, and blanking time to
    ///    CAL1_L / CAL1_H
    /// 4. Issue CTRL9 command 0x08 (CTRL_CMD_WRITE_WOM_SETTING)
    /// 5. Enable the accelerometer (CTRL7.aEN = 1)
    ///
    /// INT1 starts at low and toggles high on each WoM event.
    /// Reading STATUS1 clears the WoM flag and resets INT1 to low.
    pub fn enter_wom_mode(&mut self, i2c: &mut impl I2c) {
        // Step 1: disable sensors
        if self.imu.disable_all(i2c).is_err() {
            log::error!("IMU: failed to disable sensors for WoM");
            return;
        }

        // Step 2: lower accelerometer ODR to 31.25 Hz.
        // At 4x slower sampling than 125 Hz, per-sample slopes from
        // micro-vibrations (USB cable, desk noise) are ~4x smaller,
        // so a threshold that rejects them still admits real motion.
        if self.imu.set_accel_odr(i2c, Odr::Hz31_25).is_err() {
            log::warn!("IMU: failed to set accel ODR for WoM");
        }

        // Step 3 & 4: configure WoM with threshold 80 mg, INT1 low
        // initial value, 63 sample blanking (~2s at 31.25 Hz).
        // At 31.25 Hz, slopes from micro-vibrations are much smaller
        // than at 125 Hz, so 80 mg should catch real motion without
        // USB cable noise triggering constantly.
        let wom_cfg = WomConfig {
            threshold_mg: 80,
            interrupt: WomInterrupt::Int1Low,
            blanking_samples: 63,
        };
        if self.imu.configure_wom(i2c, &wom_cfg).is_err() {
            log::error!("IMU: WoM configuration failed");
            return;
        }

        // Step 5: enable accelerometer only
        if self.imu.set_accel_enable(i2c, true).is_err() {
            log::error!("IMU: failed to enable accel for WoM");
            return;
        }

        log::info!(
            "IMU: WoM enabled ({} mg threshold, 31.25 Hz accel ODR)",
            wom_cfg.threshold_mg,
        );
    }

    /// Exit Wake-on-Motion mode per QMI8658C datasheet section 9.6:
    ///
    /// 1. Read STATUS1 to clear any pending WoM flag
    /// 2. Disable sensors and write threshold 0 (disable_wom does both)
    /// 3. Re-initialize the IMU with the normal config (restores
    ///    accel ODR/scale, gyro, LPF, and re-enables both sensors)
    pub fn exit_wom_mode(&mut self, i2c: &mut impl I2c) {
        // Clear any pending WoM flag and reset INT1 to initial value.
        let _ = self.imu.wom_event(i2c);

        // Disable WoM (zero threshold, issue CTRL9 0x08).
        if self.imu.disable_wom(i2c).is_err() {
            log::warn!("IMU: failed to disable WoM");
        }

        // Re-init with normal config (restores ODR, scale, LPF, and
        // re-enables both accel and gyro).
        let cfg = ImuConfig::default();
        if self.imu.init(i2c, &cfg).is_err() {
            log::error!("IMU: re-init after WoM failed");
        } else {
            log::info!("IMU: WoM mode exited");
        }
    }
}
