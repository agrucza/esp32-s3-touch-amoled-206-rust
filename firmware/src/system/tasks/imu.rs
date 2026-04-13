//! IMU (QMI8658C) task state.
//!
//! Note: the **C** variant targets automotive / industrial /
//! drone applications. It provides raw 6-axis readings,
//! AttitudeEngine sensor fusion, FIFO, and Wake-on-Motion -
//! but no activity-detection engine, so no tap detection, no
//! pedometer, and no any/no/significant-motion classification
//! (those live in the A variant aimed at wearables).
//!
//! Owns the QMI8658 driver plus the INT1 line (GPIO21). The IMU
//! operates in two modes driven by the system sleep state:
//!
//!   * **Awake**: accel+gyro at 125 Hz, motion snapshots emitted
//!     on a periodic cadence so screens that display live
//!     readings (e.g. the Status screen's accel/gyro bars) stay
//!     current. INT1 is unused.
//!   * **Sleeping**: accel-only at 31.25 Hz with Wake-on-Motion
//!     configured. A motion event toggles INT1 high and the task
//!     sleeps on `wait_for_int` until that happens, emitting a
//!     single `WakeOnMotion` event.
//!
//! ## Phase 4 task loop sketch
//!
//! The task has two modes, toggled via a `Signal<SleepState>`
//! that the main task publishes when entering/exiting sleep.
//!
//! ```ignore
//! #[embassy_executor::task]
//! async fn imu_task(bus: &'static SharedI2c, mut state: ImuTaskState<'static>) {
//!     loop {
//!         match SLEEP_SIGNAL.wait_current().await {
//!             SleepState::Awake => {
//!                 // Race: either the next periodic tick, or
//!                 // a sleep-state change that pre-empts us.
//!                 match select(
//!                     Timer::after(Duration::from_millis(50)),
//!                     SLEEP_SIGNAL.wait(),
//!                 ).await {
//!                     Either::First(_) => {
//!                         let mut i2c = bus.lock().await;
//!                         let data = state.snapshot(&mut *i2c);
//!                         drop(i2c);
//!                         EVENTS.send(SystemEvent::MotionUpdated { data }).await;
//!                     }
//!                     Either::Second(_) => {} // state change, loop around
//!                 }
//!             }
//!             SleepState::Sleeping => {
//!                 // IMU is already in WoM mode (main task
//!                 // reconfigured it before signalling).
//!                 state.wait_for_int().await;
//!                 let mut i2c = bus.lock().await;
//!                 let _ = state.imu.wom_event(&mut *i2c);
//!                 drop(i2c);
//!                 EVENTS.send(SystemEvent::WakeOnMotion).await;
//!             }
//!         }
//!     }
//! }
//! ```

use drivers::imu::{ImuData, Qmi8658, Config as ImuConfig, Odr, WomConfig, WomInterrupt};
use embassy_time::{Duration, Timer};
use embedded_hal::i2c::I2c as I2cTrait;
use esp_hal::gpio::Input;

// ---- Wake-on-Motion tunables ----------------------------------------------------

/// Accelerometer ODR while in Wake-on-Motion sleep. A low ODR
/// reduces per-sample slopes from micro-vibrations (USB cable,
/// desk noise) so the threshold can reject noise while still
/// catching real wrist motion.
const WOM_ACCEL_ODR: Odr = Odr::Hz31_25;

/// Motion threshold in milli-g for the WoM engine. Slopes
/// smaller than this are ignored. 80 mg filters out table
/// vibrations; real wrist motion produces much larger slopes.
const WOM_THRESHOLD_MG: u8 = 80;

/// Which interrupt pin WoM drives, and its idle-state value.
/// INT1 starts low and toggles high on each motion event;
/// reading STATUS1 resets it.
const WOM_INTERRUPT: WomInterrupt = WomInterrupt::Int1Low;

/// Blanking time after WoM enable, in accelerometer samples.
/// 63 is the max of the 6-bit blanking field - at 31.25 Hz
/// that's about 2 s, enough to skip power-up transients.
const WOM_BLANKING_SAMPLES: u8 = 63;

/// Number of samples averaged for the initial gyro bias at
/// boot. At 125 Hz that's ~512 ms; the device must be held
/// still during this window.
const GYRO_BIAS_SAMPLES: u8 = 64;

/// Motion state (accel + gyro + IMU temperature) consumed by
/// screens that render raw sensor values. Raw signed 16-bit
/// values; convert to physical units using the configured
/// full-scale range from `ImuConfig`.
#[derive(Debug, Clone, Copy, Default)]
pub struct MotionData {
    pub accel_x: i16,
    pub accel_y: i16,
    pub accel_z: i16,
    pub gyro_x: i16,
    pub gyro_y: i16,
    pub gyro_z: i16,
    /// Raw temperature reading from the IMU die; divide by 256
    /// for degrees Celsius.
    pub temp_raw: i16,
}

impl From<&ImuData> for MotionData {
    fn from(d: &ImuData) -> Self {
        Self {
            accel_x: d.accel_x,
            accel_y: d.accel_y,
            accel_z: d.accel_z,
            gyro_x: d.gyro_x,
            gyro_y: d.gyro_y,
            gyro_z: d.gyro_z,
            temp_raw: d.temp_raw,
        }
    }
}

pub struct ImuTaskState<'d> {
    pub imu: Qmi8658,
    int_pin: Input<'d>,
}

impl<'d> ImuTaskState<'d> {
    /// Soft-reset the chip, configure the default accel/gyro, and
    /// collect a gyroscope bias estimate. The device must be held
    /// still for ~500 ms during this call.
    pub async fn init(int_pin: Input<'d>, i2c: &mut impl I2cTrait) -> Self {
        log::info!("IMU: initializing QMI8658C...");
        let imu_config = ImuConfig::default();
        let mut imu = Qmi8658::new(ImuConfig::default());

        // Soft reset to clear any leftover state from a previous
        // boot (WoM config, pedometer, etc.).
        if imu.soft_reset(i2c).is_ok() {
            log::info!("IMU: soft reset OK");
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

        Self { imu, int_pin }
    }

    /// Read a single IMU snapshot and return it as `MotionData`.
    /// Returns `Default` (all zeros) if the I2C read fails.
    ///
    /// Called periodically by the IMU task while awake; the
    /// result is sent via the event channel so the main loop
    /// can update `cached_data.motion` for live-display
    /// screens like the accelerometer visualizer.
    pub fn snapshot(&mut self, i2c: &mut impl I2cTrait) -> MotionData {
        self.imu.read(i2c).ok().as_ref().map(MotionData::from).unwrap_or_default()
    }

    /// Raw driver-level read. Kept for places that want access
    /// to the un-converted `ImuData` (e.g. calibration routines).
    pub fn read(&mut self, i2c: &mut impl I2cTrait) -> Option<ImuData> {
        self.imu.read(i2c).ok()
    }

    /// Enter Wake-on-Motion mode per QMI8658C datasheet section 9.4:
    ///
    /// 1. Disable sensors (CTRL7 = 0x00)
    /// 2. Switch accelerometer ODR to 31.25 Hz (low power)
    /// 3. Write WoM threshold, interrupt pin, and blanking time
    /// 4. Issue CTRL9 0x08 (CTRL_CMD_WRITE_WOM_SETTING)
    /// 5. Enable the accelerometer
    pub fn enter_wom_mode(&mut self, i2c: &mut impl I2cTrait) {
        if self.imu.disable_all(i2c).is_err() {
            log::error!("IMU: failed to disable sensors for WoM");
            return;
        }

        if self.imu.set_accel_odr(i2c, Odr::Hz31_25).is_err() {
            log::warn!("IMU: failed to set accel ODR for WoM");
        }

        // 80 mg threshold, INT1 low initial value, 63 sample
        // blanking (~2 s at 31.25 Hz, the max of the 6-bit field).
        let wom_cfg = WomConfig {
            threshold_mg: 80,
            interrupt: WomInterrupt::Int1Low,
            blanking_samples: 63,
        };
        if self.imu.configure_wom(i2c, &wom_cfg).is_err() {
            log::error!("IMU: WoM configuration failed");
            return;
        }

        if self.imu.set_accel_enable(i2c, true).is_err() {
            log::error!("IMU: failed to enable accel for WoM");
            return;
        }

        log::info!(
            "IMU: WoM enabled ({} mg threshold, 31.25 Hz accel ODR)",
            wom_cfg.threshold_mg,
        );
    }

    /// Exit Wake-on-Motion mode per datasheet section 9.6 and
    /// re-initialize with the default 125 Hz accel+gyro config.
    pub fn exit_wom_mode(&mut self, i2c: &mut impl I2cTrait) {
        // Clear any pending WoM flag and reset INT1 to its initial value.
        let _ = self.imu.wom_event(i2c);

        // Disable WoM (zero threshold, issue CTRL9 0x08).
        if self.imu.disable_wom(i2c).is_err() {
            log::warn!("IMU: failed to disable WoM");
        }

        // Re-init with normal config (restores ODR, scale, LPF, enables).
        let cfg = ImuConfig::default();
        if self.imu.init(i2c, &cfg).is_err() {
            log::error!("IMU: re-init after WoM failed");
        } else {
            log::info!("IMU: WoM mode exited");
        }
    }

    /// Read STATUS1 to check if a WoM event is latched. Reading
    /// also clears the flag and resets INT1 to its initial value.
    pub fn wom_event(&self, i2c: &mut impl I2cTrait) -> bool {
        self.imu.wom_event(i2c).unwrap_or(false)
    }

    /// Async wait for an IMU interrupt. INT1 is used for WoM wake
    /// while sleeping; awakened by a rising edge. In Phase 4 this
    /// becomes the wait inside the IMU task loop.
    pub async fn wait_for_int(&mut self) {
        self.int_pin.wait_for_rising_edge().await;
    }
}
