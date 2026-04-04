use crate::events::SystemEvent;
use drivers::imu::{Qmi8658, Config as ImuConfig};
use drivers::rtc::{Rtc, Config as RtcConfig, DateTime as RtcDateTime};
use embedded_hal::i2c::I2c;
use embassy_time::{Duration, Timer};

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
}
