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
//!     current.
//!   * **Sleeping**: accel-only at 31.25 Hz with Wake-on-Motion
//!     configured. The task polls STATUS1.WOM over I2C and emits
//!     a `WakeOnMotion` event when the bit sets.
//!
//! ## Why we poll STATUS1 instead of waiting on INT1
//!
//! On paper, the QMI8658C can signal WoM events on the INT1 pin
//! (selectable via CAL1_H bits 7:6). In practice, on this silicon
//! revision (`rev=0x7C`) and this board, the chip sets STATUS1.WOM
//! internally but never drives the INT1 output pin for WoM events,
//! regardless of CAL1_H polarity, CTRL8 handshake routing, or CTRL8
//! bit 6 (motion-event pin select). Both polarities and both pull
//! configurations were verified via a debug poll loop that read the
//! raw pin level alongside the STATUS1 register - the pin stayed at
//! its pull-resistor level across hundreds of polls including
//! deliberate shakes, while STATUS1.WOM toggled reliably.
//!
//! The original driver in commit 5502388 already shipped with a
//! timeout-driven STATUS1 fallback poll next to the edge wait,
//! which strongly suggests the author hit the same silicon behavior
//! then. Polling is the supported wake path per datasheet section
//! 9.3 ("When a Wake on Motion event is detected the QMI8658C will
//! set bit 2 (WoM) in the STATUS1 register. Reading STATUS1 [...]
//! will clear the WoM bit").

use crate::events::{NUM_SELF_TESTS, SelfTestError, SelfTestId, SelfTestResult, SystemEvent};
use crate::system::bus::{EVENTS, IMU_COMMAND, ImuCommand, SleepState, SharedI2c, SLEEP_WATCH};
use drivers::imu::{ImuData, Qmi8658, Config as ImuConfig, Odr, WomConfig, WomInterrupt};
use embassy_futures::select::{select, select3, Either, Either3};
use embassy_time::{Duration, Timer};
use embedded_hal::i2c::I2c as I2cTrait;
use esp_hal::gpio::Input;

/// Periodic IMU snapshot cadence while awake. 50 ms = 20 Hz,
/// plenty for live sensor-display screens without hammering the
/// I2C bus.
const AWAKE_POLL_MS: u64 = 50;

/// STATUS1.WOM poll cadence while sleeping. 500 ms = 2 Hz is a
/// compromise between wake latency and I2C traffic / power. The
/// WoM blanking filter is ~2 s after accel enable, so faster
/// polling wouldn't help during that window anyway.
const WOM_POLL_MS: u64 = 500;

/// IMU task: two modes driven by [`SLEEP_WATCH`]. Awake it
/// emits `MotionUpdated` events at 20 Hz; sleeping it arms WoM
/// and polls STATUS1.WOM at 2 Hz, emitting `WakeOnMotion` on set.
#[embassy_executor::task]
pub async fn imu_task(bus: &'static SharedI2c, mut state: ImuTaskState<'static>) {
    // Replay the boot-time self-test results once, so whichever
    // screen is interested can pick them up from `cached_data`
    // without having to re-run the tests on first open.
    for id in [SelfTestId::ImuAccel, SelfTestId::ImuGyro] {
        let result = state.self_tests[id as usize];
        EVENTS.send(SystemEvent::SelfTestUpdated { id, result }).await;
    }

    // Subscribe once; reused for both the awake and sleeping
    // branches of the task's main loop.
    let mut sleep_rx = SLEEP_WATCH
        .receiver()
        .expect("IMU: no SLEEP_WATCH receiver slot available");

    let mut sleep_state = SleepState::Awake;
    loop {
        match sleep_state {
            SleepState::Awake => {
                match select3(
                    Timer::after(Duration::from_millis(AWAKE_POLL_MS)),
                    sleep_rx.changed(),
                    IMU_COMMAND.wait(),
                ).await {
                    Either3::First(_) => {
                        let data = {
                            let mut i2c = bus.lock().await;
                            state.snapshot(&mut *i2c)
                        };
                        EVENTS.send(SystemEvent::MotionUpdated { data }).await;
                    }
                    Either3::Second(new) => {
                        sleep_state = new;
                        if new == SleepState::Sleeping {
                            let mut i2c = bus.lock().await;
                            state.enter_wom_mode(&mut *i2c);
                        }
                    }
                    Either3::Third(cmd) => {
                        handle_command(bus, &mut state, cmd).await;
                    }
                }
            }
            SleepState::Sleeping => {
                // Poll STATUS1.WOM at WOM_POLL_MS cadence. See the
                // module-level "Why we poll STATUS1 instead of waiting
                // on INT1" section for the reasoning - the short form
                // is that the INT1 output pin doesn't fire for WoM on
                // this silicon, even though the register bit does.
                let wom_fired = async {
                    loop {
                        Timer::after(Duration::from_millis(WOM_POLL_MS)).await;
                        let mut i2c = bus.lock().await;
                        if state.wom_event(&mut *i2c) {
                            break;
                        }
                    }
                };
                match select(wom_fired, sleep_rx.changed()).await {
                    Either::First(_) => {
                        EVENTS.send(SystemEvent::WakeOnMotion).await;
                    }
                    Either::Second(new) => {
                        sleep_state = new;
                        if new == SleepState::Awake {
                            let mut i2c = bus.lock().await;
                            state.exit_wom_mode(&mut *i2c);
                        }
                    }
                }
            }
        }
    }
}

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
#[allow(dead_code)]
const GYRO_BIAS_SAMPLES: u8 = 64;

// `MotionData` (struct + Default + From<&ImuData>) lives in
// `app_core::data`. Re-exported so `crate::system::tasks::imu::
// MotionData` imports in firmware keep resolving.
pub use app_core::data::MotionData;

pub struct ImuTaskState<'d> {
    pub imu: Qmi8658,
    int_pin: Input<'d>,
    /// Last known result of each IMU-owned self-test, indexed by
    /// `SelfTestId as usize`. Populated during [`init`] with the
    /// boot-time run; updated in place by [`handle_command`] when
    /// a UI re-run request comes through [`IMU_COMMAND`].
    ///
    /// [`init`]: ImuTaskState::init
    self_tests: [SelfTestResult; NUM_SELF_TESTS],
}

impl<'d> ImuTaskState<'d> {
    /// Soft-reset the chip, configure the default accel/gyro, and
    /// collect a gyroscope bias estimate. The device must be held
    /// still for ~500 ms during this call. Also runs the datasheet
    /// self-tests once and stashes their results in `self_tests`
    /// so the task can replay them over the event bus on startup.
    pub async fn init(int_pin: Input<'d>, i2c: &mut impl I2cTrait) -> Self {
        log::info!("IMU: initializing QMI8658C...");
        let imu_config = ImuConfig::default();
        let mut imu = Qmi8658::new(ImuConfig::default());
        let mut self_tests: [SelfTestResult; NUM_SELF_TESTS] =
            [SelfTestResult::NotRun; NUM_SELF_TESTS];

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

                // Settling delay before the first self-test. 100 ms
                // (the previous value) is enough for a re-run hours
                // into a boot but not enough right after the soft
                // reset + init sequence - the first self-test after
                // boot times out on STATUSINT.bit0. 250 ms reliably
                // clears that window. The second test (run right
                // after) doesn't need another settling delay because
                // the chip has been running continuously by then.
                Timer::after(Duration::from_millis(250)).await;

                // Run both self-tests per datasheet section 11. These
                // leave CTRL7 = 0 and touch CTRL2/CTRL3, so we re-run
                // init() afterwards to restore the normal config. The
                // results are stashed in `self_tests` so `imu_task`
                // can replay them as `SelfTestUpdated` events once it
                // starts (see the top of `imu_task`).
                self_tests[SelfTestId::ImuAccel as usize] = match imu.run_accel_self_test(i2c) {
                    Ok(r) => {
                        log::info!(
                            "IMU: accel self-test {} [{} {} {}] mg",
                            if r.passed { "PASS" } else { "FAIL" },
                            r.x_mg, r.y_mg, r.z_mg,
                        );
                        let values = [r.x_mg, r.y_mg, r.z_mg];
                        if r.passed { SelfTestResult::PassAxes3(values) }
                        else { SelfTestResult::FailAxes3(values) }
                    }
                    Err(_) => {
                        log::warn!("IMU: accel self-test failed to complete");
                        SelfTestResult::Error(SelfTestError::Timeout)
                    }
                };
                self_tests[SelfTestId::ImuGyro as usize] = match imu.run_gyro_self_test(i2c) {
                    Ok(r) => {
                        log::info!(
                            "IMU: gyro self-test {} [{} {} {}] dps",
                            if r.passed { "PASS" } else { "FAIL" },
                            r.x_dps, r.y_dps, r.z_dps,
                        );
                        let values = [r.x_dps, r.y_dps, r.z_dps];
                        if r.passed { SelfTestResult::PassAxes3(values) }
                        else { SelfTestResult::FailAxes3(values) }
                    }
                    Err(_) => {
                        log::warn!("IMU: gyro self-test failed to complete");
                        SelfTestResult::Error(SelfTestError::Timeout)
                    }
                };
                if imu.init(i2c, &imu_config).is_err() {
                    log::error!("IMU: re-init after self-test failed");
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

        Self { imu, int_pin, self_tests }
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
    #[allow(dead_code)]
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

        if self.imu.set_accel_odr(i2c, WOM_ACCEL_ODR).is_err() {
            log::warn!("IMU: failed to set accel ODR for WoM");
        }

        let wom_cfg = WomConfig {
            threshold_mg: WOM_THRESHOLD_MG,
            interrupt: WOM_INTERRUPT,
            blanking_samples: WOM_BLANKING_SAMPLES,
        };
        if self.imu.configure_wom(i2c, &wom_cfg).is_err() {
            log::error!("IMU: WoM configuration failed");
            return;
        }

        if self.imu.set_accel_enable(i2c, true).is_err() {
            log::error!("IMU: failed to enable accel for WoM");
            return;
        }

        // Clear any stale STATUS1.WOM bit left over from a previous
        // sleep cycle so the poll loop doesn't fire immediately.
        let _ = self.imu.wom_event(i2c);

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

    /// Async wait for an IMU interrupt. Not used by the WoM wake
    /// path (see module docs for why) but kept in case INT1 is
    /// ever repurposed - e.g. for CTRL9 handshake if the driver
    /// stops routing through STATUSINT.bit7, or for data-ready.
    #[allow(dead_code)]
    pub async fn wait_for_int(&mut self) {
        self.int_pin.wait_for_rising_edge().await;
    }

    /// Run one specific self-test by id, synchronously on the
    /// caller's I2C lock. Returns the new [`SelfTestResult`] without
    /// touching `self.self_tests` - the caller decides whether to
    /// cache the result (command handler does; boot-time init
    /// already has its own storage path).
    fn run_self_test(&mut self, i2c: &mut impl I2cTrait, id: SelfTestId) -> SelfTestResult {
        let result = match id {
            SelfTestId::ImuAccel => match self.imu.run_accel_self_test(i2c) {
                Ok(r) => {
                    let values = [r.x_mg, r.y_mg, r.z_mg];
                    if r.passed { SelfTestResult::PassAxes3(values) }
                    else { SelfTestResult::FailAxes3(values) }
                }
                Err(_) => SelfTestResult::Error(SelfTestError::Timeout),
            },
            SelfTestId::ImuGyro => match self.imu.run_gyro_self_test(i2c) {
                Ok(r) => {
                    let values = [r.x_dps, r.y_dps, r.z_dps];
                    if r.passed { SelfTestResult::PassAxes3(values) }
                    else { SelfTestResult::FailAxes3(values) }
                }
                Err(_) => SelfTestResult::Error(SelfTestError::Timeout),
            },
        };

        // Both self-tests leave sensors disabled and CTRL2/CTRL3
        // partially modified - restore the normal config before
        // handing the bus back to the caller.
        let cfg = ImuConfig::default();
        if self.imu.init(i2c, &cfg).is_err() {
            log::error!("IMU: re-init after self-test failed");
        }
        result
    }
}

/// Handle one [`ImuCommand`] received on the [`IMU_COMMAND`] signal.
///
/// Lives outside the `impl` block because it needs access to
/// `bus`/`EVENTS` to drive the Running → Pass/Fail event sequence,
/// which the task state struct doesn't own.
async fn handle_command(
    bus: &'static SharedI2c,
    state: &mut ImuTaskState<'static>,
    cmd: ImuCommand,
) {
    match cmd {
        ImuCommand::RunSelfTest(id) => {
            // Emit Running first so the screen can dim the card
            // before the bus lock holds up any redraws.
            EVENTS.send(SystemEvent::SelfTestUpdated {
                id,
                result: SelfTestResult::Running,
            }).await;

            // Lock the bus, run the test, release the lock before
            // sending the result event so the main loop can run its
            // dispatch without waiting on the I2C mutex.
            let result = {
                let mut i2c = bus.lock().await;
                state.run_self_test(&mut *i2c, id)
            };

            // Cache locally so the next post-wake replay shows the
            // latest result rather than the stale boot-time one.
            state.self_tests[id as usize] = result;

            EVENTS.send(SystemEvent::SelfTestUpdated { id, result }).await;
        }
    }
}
