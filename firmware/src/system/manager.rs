extern crate alloc;

use crate::config::Config;
use crate::display_hal::{self, WIDTH, HEIGHT};
use crate::events::{SelfTestId, SystemEvent};
use crate::sdcard_hal::EspVolumeManager;
use crate::system::audio::AudioSystem;
use crate::system::bus::{EVENTS, IMU_COMMAND, ImuCommand, RTC_COMMAND, RtcCommand, SLEEP_WATCH, SleepState};
use crate::system::display::Display;
use crate::system::power::PowerControls;
use crate::system::tasks::boot_button::BootButtonTaskState;
use crate::system::tasks::imu::ImuTaskState;
use crate::system::tasks::power::PowerTaskState;
use crate::system::tasks::rtc::RtcTaskState;
use crate::system::tasks::touch::TouchTaskState;
use crate::ui::primitives;
use crate::ui::types::SystemData;
use embedded_graphics::draw_target::DrawTarget;
use embassy_futures::select::{select, Either};
use embassy_time::{Duration, Instant, Timer, with_timeout};
use esp_hal::{
    dma::DmaDescriptor,
    gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull},
    i2c::master::{Config as I2cConfig, I2c},
    i2s::master::asynch::{I2sReadDmaTransferAsync, I2sWriteDmaTransferAsync},
    time::Rate,
};

// -- CPU frequency scaling ---------------------------------------------------

/// CPU frequency levels for dynamic scaling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuFreq {
    /// 80 MHz - baseline for idle/low-power operation.
    Mhz80,
    /// 160 MHz - mid-range for moderate workloads.
    Mhz160,
    /// 240 MHz - full speed for rendering and heavy computation.
    /// Not currently used at runtime (we boost to 160 MHz for
    /// render which is enough); kept for future codepaths that
    /// need absolute max clock.
    #[allow(dead_code)]
    Mhz240,
}

/// Switch CPU frequency at runtime. APB stays at 80 MHz for all
/// settings so peripheral clocks (I2C, SPI) are unaffected.
/// Embassy timers use the XTAL-based systimer and are also unaffected.
///
/// Note: `esp_hal::clock::cpu_clock()` will keep reporting the
/// init-time CPU clock (240 MHz with `CpuClock::max()`) regardless
/// of what we write here. esp-hal caches that value in a static
/// `Clocks` struct at init and never re-reads the register. The
/// silicon is actually scaling correctly; only esp-hal's view is
/// stale. Verified on esp-hal 1.1.0-rc.0 by reading cpu_per_conf
/// back after each write.
fn set_cpu_freq(freq: CpuFreq) {
    use esp_hal::peripherals::SYSTEM;

    let (period_sel, freq_mhz) = match freq {
        CpuFreq::Mhz80 => (0u8, 80u32),
        CpuFreq::Mhz160 => (1u8, 160u32),
        CpuFreq::Mhz240 => (2u8, 240u32),
    };

    SYSTEM::regs().cpu_per_conf().modify(|_, w| unsafe {
        w.pll_freq_sel().set_bit();
        w.cpuperiod_sel().bits(period_sel)
    });

    esp_hal::rom::ets_update_cpu_frequency_rom(freq_mhz);
}

// Type aliases for complex generic types. `Display<'d>` lives in
// `system::display` since it's fundamentally a display concern.
pub type AudioTx<'d> = I2sWriteDmaTransferAsync<'d, &'static mut [u8]>;
pub type AudioRx<'d> = I2sReadDmaTransferAsync<'d, &'static mut [u8]>;

/// Size of the per-tick mic RX drain buffer. Matches the I2S RX DMA
/// buffer in `main.rs` so one `pop()` can empty a full buffer.
const MIC_DRAIN_BUF_SIZE: usize = 16_384;

/// Peripheral tokens for the audio subsystem, stashed on
/// `SystemManager` until `start_audio()` consumes them. Keeping the
/// whole bundle in one struct lets us `.take()` it as a single move
/// when audio is eventually brought online, without re-plumbing
/// peripherals from outside the manager.
///
/// The audio subsystem is intentionally NOT started in `init()` so the
/// ES8311 DAC, ES7210 ADC, I2S DMA, MCLK, and speaker amp all stay in
/// their post-reset low-power state. When a feature needs audio, it
/// calls `SystemManager::start_audio()` which wires everything up via
/// `crate::system::audio::init_audio`.
struct PendingAudio<'d> {
    i2s0: esp_hal::peripherals::I2S0<'d>,
    dma_ch1: esp_hal::peripherals::DMA_CH1<'d>,
    audio_mclk: esp_hal::peripherals::GPIO16<'d>,
    audio_bclk: esp_hal::peripherals::GPIO41<'d>,
    audio_ws: esp_hal::peripherals::GPIO45<'d>,
    audio_dout: esp_hal::peripherals::GPIO40<'d>,
    audio_din: esp_hal::peripherals::GPIO42<'d>,
    audio_pa: esp_hal::peripherals::GPIO46<'d>,
    tx_buffer: &'static mut [u8],
    rx_buffer: &'static mut [u8],
    tx_descriptors: &'static mut [DmaDescriptor],
    rx_descriptors: &'static mut [DmaDescriptor],
}

/// Framebuffer row stride in bytes. Used by the dirty-row flush path.
const ROW_STRIDE: usize = WIDTH as usize * 2;

/// FNV-1a 32-bit hash of one framebuffer row. Rows are `WIDTH * 2`
/// bytes = 820 bytes = 205 u32 words, so we process the row as u32
/// chunks (the row width is hardware-fixed to an even number of u32s,
/// so there is no tail to worry about).
#[inline]
fn row_hash(row: &[u8]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for chunk in row.chunks_exact(4) {
        let v = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        h ^= v;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// Bundle of per-device task state structs produced by
/// [`SystemManager::init`] and handed to `main` so it can spawn
/// one embassy task per peripheral.
///
/// Each field owns everything a task needs to run independently
/// (driver handle, GPIO lines, buffers). The shared I2C bus is
/// handed to each task separately as a `&'static SharedI2c` via
/// [`SystemManager::i2c_bus`] and is intentionally not part of
/// this bundle.
pub struct TaskBundle {
    pub touch: TouchTaskState<'static>,
    pub boot_button: BootButtonTaskState<'static>,
    pub rtc: RtcTaskState<'static>,
    pub imu: ImuTaskState<'static>,
    pub power: PowerTaskState,
}

// `audio`, `storage`, and `tx_transfer` are held-but-not-read once
// populated: they own hardware resources whose Drop impls would
// deinitialize the peripherals. They stay as fields so the hardware
// keeps running after `start_audio` brings them online.
#[allow(dead_code)]
pub struct SystemManager<'d> {
    // Shared I2C bus behind an async mutex. Lives in the global
    // `I2C_BUS` StaticCell; tasks and the main loop lock it before
    // each access. See `system::bus` for details.
    i2c_bus: &'static crate::system::bus::SharedI2c,

    // Non-I2C power hardware (SYS_OUT latch, haptic motor). The
    // I2C side of the PMU lives in the power task.
    pub power: PowerControls<'d>,

    // Peripherals
    pub display: Display<'d>,
    pub audio: Option<AudioSystem<'d>>,
    pub storage: Option<EspVolumeManager<'d>>,

    // Tearing-effect line from the display. We wait for its rising
    // edge (vblank start) before pushing pixels so partial flushes
    // don't land mid-scanout.
    lcd_te: Input<'d>,

    // DMA transfers - populated by `start_audio`, `None` until then.
    tx_transfer: Option<AudioTx<'d>>,
    rx_transfer: Option<AudioRx<'d>>,

    // Raw audio peripheral tokens stashed at boot, consumed by
    // `start_audio`. `None` after audio has been started once.
    pending_audio: Option<PendingAudio<'d>>,

    // UI + app state: screen, nav stack, sleep flag, display
    // state, cached snapshots, dim/idle timers, buzz pattern,
    // config. All moved into `app_core::model::Model` where the
    // event->effect dispatch is host-testable.
    model: app_core::model::Model,

    // Periodic loop counter (diagnostics).
    tick_count: u32,

    // Pre-allocated drain buffer for the mic RX DMA ring.
    mic_drain_buf: alloc::boxed::Box<[u8]>,

    // Per-row FNV-1a hashes from the previous frame. Compared against
    // the current frame to determine which rows actually changed, so
    // only the dirty horizontal band is pushed over QSPI.
    row_hashes: alloc::boxed::Box<[u32]>,

    // RTC controller, used to enter/exit hardware light sleep.
    rtc: esp_hal::rtc_cntl::Rtc<'d>,

}

// `NAV_STACK_DEPTH` and the `NavStack` type live in
// `app_core::nav` so the stack's push/pop semantics are
// host-testable.

/// All peripheral tokens needed by the system manager.
///
/// Groups the raw esp-hal peripheral tokens so `SystemManager::init()` has
/// a single parameter instead of 30+ individual pins. Created in main()
/// right after `esp_hal::init()`.
pub struct Peripherals<'d> {
    // I2C bus
    pub i2c0: esp_hal::peripherals::I2C0<'d>,
    pub i2c_sda: esp_hal::peripherals::GPIO15<'d>,
    pub i2c_scl: esp_hal::peripherals::GPIO14<'d>,

    // Power
    pub sys_out_pin: esp_hal::peripherals::GPIO10<'d>,
    pub motor_pin: esp_hal::peripherals::GPIO18<'d>,

    // Display
    pub spi2: esp_hal::peripherals::SPI2<'d>,
    pub lcd_sclk: esp_hal::peripherals::GPIO11<'d>,
    pub lcd_sio0: esp_hal::peripherals::GPIO4<'d>,
    pub lcd_sio1: esp_hal::peripherals::GPIO5<'d>,
    pub lcd_sio2: esp_hal::peripherals::GPIO6<'d>,
    pub lcd_sio3: esp_hal::peripherals::GPIO7<'d>,
    pub lcd_cs: esp_hal::peripherals::GPIO12<'d>,
    pub dma_ch0: esp_hal::peripherals::DMA_CH0<'d>,
    pub lcd_reset: esp_hal::peripherals::GPIO8<'d>,
    pub lcd_te: esp_hal::peripherals::GPIO13<'d>,

    // Input
    pub btn_boot: esp_hal::peripherals::GPIO0<'d>,
    pub touch_rst: esp_hal::peripherals::GPIO9<'d>,
    pub touch_int: esp_hal::peripherals::GPIO38<'d>,
    pub rtc_int: esp_hal::peripherals::GPIO39<'d>,
    pub imu_int1: esp_hal::peripherals::GPIO21<'d>,

    // SD card
    pub spi3: esp_hal::peripherals::SPI3<'d>,
    pub sd_sck: esp_hal::peripherals::GPIO2<'d>,
    pub sd_mosi: esp_hal::peripherals::GPIO1<'d>,
    pub sd_miso: esp_hal::peripherals::GPIO3<'d>,
    pub sd_cs: esp_hal::peripherals::GPIO17<'d>,

    // Audio
    pub i2s0: esp_hal::peripherals::I2S0<'d>,
    pub dma_ch1: esp_hal::peripherals::DMA_CH1<'d>,
    pub audio_mclk: esp_hal::peripherals::GPIO16<'d>,
    pub audio_bclk: esp_hal::peripherals::GPIO41<'d>,
    pub audio_ws: esp_hal::peripherals::GPIO45<'d>,
    pub audio_dout: esp_hal::peripherals::GPIO40<'d>,
    pub audio_din: esp_hal::peripherals::GPIO42<'d>,
    pub audio_pa: esp_hal::peripherals::GPIO46<'d>,

    // RTC controller (for light sleep)
    pub lpwr: esp_hal::peripherals::LPWR<'d>,

    // Audio DMA buffers (from dma_circular_buffers! macro in main)
    pub tx_buffer: &'static mut [u8],
    pub rx_buffer: &'static mut [u8],
    pub tx_descriptors: &'static mut [DmaDescriptor],
    pub rx_descriptors: &'static mut [DmaDescriptor],
}

impl SystemManager<'static> {
    /// Initialize all subsystems and assemble the system manager.
    ///
    /// This is the single entry point for the entire system. Call it from
    /// main() after HAL init, timer start, and logger setup.
    ///
    /// Requires `'static` peripherals (from `esp_hal::init`) so the
    /// I2C bus can be stored in the global `I2C_BUS` static mutex
    /// for sharing with peripheral tasks.
    ///
    /// Init order is critical:
    /// 1. I2C bus (shared by PMU, touch, IMU, RTC, codecs)
    /// 2. Power (enables all rails, must be first peripheral)
    /// 3. Framebuffer allocation (PSRAM heap is already set up
    ///    by `main` before this function is called)
    /// 4. Display
    /// 5. Input (touch + buttons)
    /// 6. SD card
    /// 7. Sensors (RTC + IMU with ~500ms gyro calibration)
    /// 8. Audio peripherals stashed into `pending_audio` but NOT
    ///    started. A caller invokes `start_audio()` later to actually
    ///    bring the codecs, I2S DMA, and speaker amp online. This
    ///    keeps idle current low on battery by default.
    ///
    /// Returns `(SystemManager, TaskBundle)` - the bundle holds the
    /// per-device task state structs that `main` then passes to
    /// `spawner.spawn()` to start the peripheral tasks.
    pub async fn init(p: Peripherals<'static>) -> (Self, TaskBundle) {
        // 1. I2C bus
        let mut i2c = I2c::new(p.i2c0, I2cConfig::default().with_frequency(Rate::from_khz(400)))
            .unwrap()
            .with_sda(p.i2c_sda)
            .with_scl(p.i2c_scl);

        // 2. Power - must init first (enables all power rails).
        // PowerControls owns the GPIO sides (sys_out latch, motor);
        // PowerTaskState owns the I2C-driver Pmu handle for polling.
        let (power, pmu) = PowerControls::init(
            Output::new(p.sys_out_pin, Level::Low, OutputConfig::default()),
            Output::new(p.motor_pin, Level::Low, OutputConfig::default()),
            &mut i2c,
        ).expect("PMU init failed - halting");
        let power_state = PowerTaskState::new(pmu);
        Timer::after(Duration::from_millis(20)).await;

        // 3. Framebuffer (PSRAM heap already initialized in `main`)
        let fb: &'static mut [u8] = alloc::vec![0u8; display_hal::FB_BYTES].leak();

        // 4. Display
        let display = crate::system::display::init_display(
            p.spi2, p.lcd_sclk, p.lcd_sio0, p.lcd_sio1,
            p.lcd_sio2, p.lcd_sio3, p.lcd_cs, p.dma_ch0,
            Output::new(p.lcd_reset, Level::High, OutputConfig::default()),
            fb,
        ).await;

        // TE (tearing-effect) input. The CO5300 init already enabled
        // TE in vblank-only mode (cmd 0x35 [0x00]); we just need to
        // watch the rising edge before each flush.
        let lcd_te = Input::new(p.lcd_te, InputConfig::default().with_pull(Pull::None));

        // 5. Touch + BOOT button task states.
        //
        // `TouchTaskState::init` does the FT3168 reset sequence,
        // reads the chip ID, and captures the INT# input. The BOOT
        // button is just an async GPIO wait.
        //
        // Arm RTC-wake on the touch INT, BOOT button, and PCF85063
        // INT lines before handing them to the tasks. `wakeup_enable`
        // sets a persistent hardware register bit; the async edge
        // waits done by the tasks coexist with it.
        let mut touch_int = Input::new(p.touch_int, InputConfig::default().with_pull(Pull::Up));
        let mut boot_btn = Input::new(p.btn_boot, InputConfig::default().with_pull(Pull::Up));
        let mut rtc_int = Input::new(p.rtc_int, InputConfig::default().with_pull(Pull::Up));
        let _ = touch_int.wakeup_enable(true, esp_hal::gpio::WakeEvent::LowLevel);
        let _ = boot_btn.wakeup_enable(true, esp_hal::gpio::WakeEvent::LowLevel);
        let _ = rtc_int.wakeup_enable(true, esp_hal::gpio::WakeEvent::LowLevel);

        let touch_state = TouchTaskState::init(
            Output::new(p.touch_rst, Level::High, OutputConfig::default()),
            touch_int,
            &mut i2c,
        ).await;
        let boot_button_state = BootButtonTaskState::new(boot_btn);

        // 6. SD card
        let storage = crate::system::storage::init_sd(
            p.spi3, p.sd_sck, p.sd_mosi, p.sd_miso,
            Output::new(p.sd_cs, Level::High, OutputConfig::default()),
        );

        // 7. Sensors (RTC + IMU). Each owns its INT line and driver.
        // `RtcTaskState::init` brings the PCF85063A up, sets a default
        // time if the oscillator stopped, and arms the half-minute
        // interrupt. `ImuTaskState::init` resets the QMI8658C and
        // collects ~512 ms of gyro-bias samples.
        let rtc_state = RtcTaskState::init(
            rtc_int,
            &mut i2c,
        );
        let imu_state = ImuTaskState::init(
            Input::new(p.imu_int1, InputConfig::default().with_pull(Pull::Down)),
            &mut i2c,
        ).await;

        // Seed the cached data so the first frame has something
        // reasonable to render before task events arrive.
        let initial_time = rtc_state.snapshot(&mut i2c);
        let initial_power = power_state.snapshot(&mut i2c);

        // 8. Stash audio peripherals for later `start_audio()`.
        // Hardware stays in post-reset low-power state until then.
        let pending_audio = Some(PendingAudio {
            i2s0: p.i2s0,
            dma_ch1: p.dma_ch1,
            audio_mclk: p.audio_mclk,
            audio_bclk: p.audio_bclk,
            audio_ws: p.audio_ws,
            audio_dout: p.audio_dout,
            audio_din: p.audio_din,
            audio_pa: p.audio_pa,
            tx_buffer: p.tx_buffer,
            rx_buffer: p.rx_buffer,
            tx_descriptors: p.tx_descriptors,
            rx_descriptors: p.rx_descriptors,
        });

        log::info!("System: all subsystems initialized (audio stashed)");

        // Dump PMU status now that all ADC channels have had time to
        // settle during display, touch, SD, and sensor init (~1 s).
        power_state.dump_status(&mut i2c);

        // Move the I2C bus into the global StaticCell-backed mutex.
        // From here on, every access goes through `i2c_bus.lock()`.
        // The `i2c` local is consumed and no longer accessible.
        let i2c_bus: &'static crate::system::bus::SharedI2c = crate::system::bus::I2C_BUS
            .init(embassy_sync::mutex::Mutex::new(i2c));

        // Seed the initial cached-data snapshot so the first frame
        // renders against sensible values before any task events
        // arrive. Default-initialize everything else.
        let mut cached_data = SystemData::default();
        cached_data.time = initial_time;
        cached_data.power = initial_power;

        // Construct the Model (UI + cached state + dispatch).
        let model = app_core::model::Model::new(
            cached_data,
            Config::default(),
            Instant::now(),
        );

        // Construct the RTC controller for hardware light sleep.
        let rtc = esp_hal::rtc_cntl::Rtc::new(p.lpwr);

        let manager = Self {
            i2c_bus,
            power,
            display,
            audio: None,
            storage,
            lcd_te,
            tx_transfer: None,
            rx_transfer: None,
            pending_audio,
            model,
            tick_count: 0,
            mic_drain_buf: alloc::vec![0u8; MIC_DRAIN_BUF_SIZE].into_boxed_slice(),
            row_hashes: alloc::vec![0u32; HEIGHT as usize].into_boxed_slice(),
            rtc,
        };

        let bundle = TaskBundle {
            touch: touch_state,
            boot_button: boot_button_state,
            rtc: rtc_state,
            imu: imu_state,
            power: power_state,
        };

        // Drop to 80 MHz baseline after init. Render boosts
        // to 160 MHz temporarily for frame throughput.
        set_cpu_freq(CpuFreq::Mhz80);

        (manager, bundle)
    }

    /// Accessor for the shared I2C bus reference.
    ///
    /// `main` needs this so it can hand the same `&'static SharedI2c`
    /// to each spawned peripheral task after `init` returns. The
    /// underlying field stays private; every runtime I2C user in
    /// manager goes through `self.i2c_bus.lock().await` directly.
    pub fn i2c_bus(&self) -> &'static crate::system::bus::SharedI2c {
        self.i2c_bus
    }

    /// Bring the audio subsystem online. Consumes the peripheral
    /// tokens stashed at boot and calls `init_audio`, which starts
    /// I2S DMA (MCLK/BCLK/LRCK output), configures the ES8311 DAC and
    /// ES7210 ADC over I2C, and enables the NS4150B speaker amp.
    ///
    /// Only succeeds once per boot - if audio has already been
    /// started (or this is called a second time), it logs and
    /// returns without touching the hardware.
    #[allow(dead_code)]
    pub async fn start_audio(&mut self) {
        let Some(pa) = self.pending_audio.take() else {
            log::warn!("Audio: start_audio called but already started");
            return;
        };
        log::info!("Audio: bringing subsystem online...");

        // Enable the audio analog rail (ALDO1 → A3V3) before any
        // codec / ADC access. The rail is held off at boot by
        // `Pmu::init` to save idle current while the audio stack
        // is dormant, so it MUST be turned on here before
        // `init_audio` touches the ES8311 or ES7210 over I²C.
        // Leaving this step out will manifest as I²C NAKs or
        // silent corruption inside the codec / ADC init routines,
        // so this block is deliberately part of `start_audio`
        // rather than a separate method the caller might forget
        // to invoke. See `Pmu::set_audio_rail` for the full
        // audio-init contract.
        {
            let mut i2c = self.i2c_bus.lock().await;
            let pmu = drivers::pmu::Pmu::new(drivers::pmu::Config::default());
            match pmu.set_audio_rail(&mut *i2c, true) {
                Ok(()) => log::info!("Audio: ALDO1 (A3V3) enabled"),
                Err(_) => log::warn!("Audio: failed to enable ALDO1 - codec/ADC init will likely fail"),
            }
        }
        // Let the LDO settle before driving I²C to the codec/ADC.
        Timer::after(Duration::from_millis(10)).await;

        let mut i2c = self.i2c_bus.lock().await;
        let (audio, tx_transfer, rx_transfer) = crate::system::audio::init_audio(
            pa.i2s0, pa.dma_ch1,
            pa.audio_mclk, pa.audio_bclk, pa.audio_ws,
            pa.audio_dout, pa.audio_din,
            Output::new(pa.audio_pa, Level::Low, OutputConfig::default()),
            pa.tx_buffer, pa.rx_buffer, pa.tx_descriptors, pa.rx_descriptors,
            &mut *i2c,
        ).await;
        drop(i2c);
        self.audio = Some(audio);
        self.tx_transfer = Some(tx_transfer);
        self.rx_transfer = Some(rx_transfer);
    }

    // ================= Effect executor ===========================================

    /// Apply the effects emitted by `Model::handle_event` /
    /// `Model::tick`. Each variant maps to the concrete hardware
    /// path (signal channel, async display transition, GPIO
    /// toggle, shutdown).
    async fn execute_effects(&mut self, effects: app_core::model::Effects) {
        use app_core::model::Effect;
        for effect in effects {
            match effect {
                Effect::TransitionDisplay { from, to } => {
                    let _ = crate::system::display::transition(
                        &mut self.display,
                        from,
                        to,
                        &self.model.config.display,
                    ).await;
                }
                Effect::BroadcastSleeping => {
                    SLEEP_WATCH.sender().send(SleepState::Sleeping);
                    log::info!("system: sleep");
                }
                Effect::BroadcastAwake => {
                    SLEEP_WATCH.sender().send(SleepState::Awake);
                    self.row_hashes.fill(0); // force full redraw next frame
                    log::info!("system: wake");
                }
                Effect::EnterLightSleep => {
                    self.enter_light_sleep();
                }
                Effect::MotorOn => self.power.buzz(),
                Effect::MotorOff => self.power.buzz_stop(),
                Effect::MotorPulse { duration_ms } => {
                    self.power.buzz();
                    Timer::after(Duration::from_millis(duration_ms as u64)).await;
                    self.power.buzz_stop();
                }
                Effect::SetAlarm { hour, minute, weekday } => {
                    RTC_COMMAND.signal(RtcCommand::SetAlarm { hour, minute, weekday });
                }
                Effect::CancelAlarm => RTC_COMMAND.signal(RtcCommand::CancelAlarm),
                Effect::StartTimer { seconds } => {
                    RTC_COMMAND.signal(RtcCommand::StartTimer { seconds });
                }
                Effect::CancelTimer => RTC_COMMAND.signal(RtcCommand::CancelTimer),
                Effect::SetTime { year, month, day, hour, minute, second } => {
                    RTC_COMMAND.signal(RtcCommand::SetTime {
                        year, month, day, hour, minute, second,
                    });
                }
                Effect::RunSelfTest(id) => {
                    match id {
                        SelfTestId::ImuAccel | SelfTestId::ImuGyro => {
                            IMU_COMMAND.signal(ImuCommand::RunSelfTest(id));
                        }
                    }
                }
                Effect::Shutdown => {
                    log::info!("System: shutdown requested");
                    self.power.shutdown();
                }
            }
        }
    }

    /// The old sleep/wake docs preserved for reference.
    ///
    /// Sleep broadcasts [`SLEEP_WATCH`] so subscribers can enter
    /// low-power modes (IMU -> WoM, touch -> Monitor, power task
    /// -> slow poll), blanks the display, and sets Model's
    /// `sleeping` flag. Wake reverses it. All of that now lives
    /// in `app_core::model::Model::{sleep, wake}` emitting
    /// effects that `execute_effects` applies here.
    // `sleep`, `wake`, `apply_dim_state`, `check_idle_sleep`,
    // `tick_buzz`, `check_alarm_reprogram` all live on
    // `app_core::model::Model` now. The manager's role shrinks to
    // `execute_effects` (above) plus the tick/render loop (below).

    // ================= Light sleep ================================================

    /// Enter hardware light sleep. Blocks until a configured wake
    /// source (any of the three RTC-armed GPIO INTs, or the ~1 s
    /// heartbeat timer) fires, then returns.
    ///
    /// The three INT pins are armed for RTC wake at init time via
    /// `wakeup_enable(true, WakeEvent::LowLevel)`. The timer is
    /// configured per-call here.
    ///
    /// Only a minimal config is used - defaults from esp-hal 1.1
    /// pick up the BBPLL-force-pu fix that was missing in 1.0.0.
    /// `light_slp_reject(false)` stops a pending interrupt (e.g.
    /// a half-minute PCF85063 INT still latched) from silently
    /// cancelling sleep entry.
    fn enter_light_sleep(&mut self) {
        use esp_hal::delay::Delay;
        use esp_hal::peripherals::GPIO;
        use esp_hal::rtc_cntl::sleep::{
            GpioWakeupSource, RtcSleepConfig, TimerWakeupSource,
        };

        // Re-arm GPIO wake for touch_int(38), boot_btn(0), and
        // rtc_int(39). The embassy async GPIO drivers used by
        // those pins' tasks call `listen_with_options(...,
        // wake_up_from_light_sleep=false)` on every wait, which
        // clears the wakeup_enable bit we set at init. We have to
        // set it back immediately before rtc.sleep(). int_type=4
        // is LowLevel, which is what the INT lines drive when
        // active (active-low on all three) and is the only type
        // esp-hal allows for wake-from-light-sleep.
        for &gpio_num in &[0u8, 38u8, 39u8] {
            GPIO::regs().pin(gpio_num as usize).modify(|_, w| unsafe {
                w.wakeup_enable().set_bit();
                w.int_type().bits(4)
            });
        }

        let gpio_wake = GpioWakeupSource::new();
        // 5 s matches the power task's sleep-mode poll interval,
        // so every wake cycle lines up with a real background poll
        // (battery, VBUS, charging state). At ~150 ms active per
        // wake this is ~3 % duty cycle - low overhead while still
        // letting background tasks make forward progress during
        // sleep. Touch/BOOT/RTC INT wake-from-sleep still fires
        // immediately regardless of this timer.
        let timer_wake = TimerWakeupSource::new(
            core::time::Duration::from_secs(5),
        );

        // Config that reliably wakes on ESP32-S3 (validated via
        // `bin/sleep_test.rs`). The critical bits:
        // - `xtal_fpu(true)` keeps the main XTAL powered, without
        //   which the CPU can't resume clocking on wake.
        // - `rtc_regulator_fpu(true)` keeps the RTC regulator on
        //   so the RTC domain stays functional during sleep.
        // - `light_slp_reject(false)` tolerates pending interrupts
        //   at sleep entry (e.g. a latched PCF85063 INT line).
        let mut config = RtcSleepConfig::default();
        config.set_rtc_regulator_fpu(true);
        config.set_xtal_fpu(true);
        config.set_light_slp_reject(false);

        // Drop to 80 MHz before sleep. At CpuClock::max() (240 MHz)
        // the slow-clock alarm programming in TimerWakeupSource
        // can't latch before the CPU enters sleep, and timer wake
        // never fires. See esp-hal issue #375 discussion thread.
        set_cpu_freq(CpuFreq::Mhz80);

        // Settle delay before sleep - lets the slow-clock alarm
        // writes latch into the RTC domain before the CPU gates
        // off. 100 ms matches the official esp-hal sleep_timer
        // example; shorter values have been observed to miss wake.
        let delay = Delay::new();
        delay.delay_millis(100);

        self.rtc.sleep(&config, &[&gpio_wake, &timer_wake]);

        // Post-wake settle delay. USB-Serial-JTAG in particular
        // loses the first ~tens of ms of output after light sleep
        // wake because the USB host has to re-sync. Without this
        // delay the first `log::info!` below gets partially eaten
        // by the host and reads as a bare "INFO - " line. Costs a
        // bit of idle power but makes on-device debugging usable;
        // drop or shrink once we're confident this is shipping.
        delay.delay_millis(100);

        log::info!("light_sleep: woke");
    }

    // ================= Event loop ================================================

    /// Handle a single event received from the global event channel.
    ///
    /// The pure logic lives on `app_core::model::Model::handle_event`
    /// (snapshot caching, sleep/wake decisions, screen dispatch,
    /// action interpretation). This wrapper hands the event to the
    /// model and executes the returned effects on hardware.
    async fn handle_event(&mut self, event: SystemEvent) {
        let effects = self.model.handle_event(&event, Instant::now());
        self.execute_effects(effects).await;
    }

    /// Run one iteration of the main event loop.
    ///
    /// Waits on the global event channel with an idle-timeout, then
    /// applies dim/idle-sleep transitions and renders if anything
    /// flagged a redraw. The timeout gives the idle timer a heartbeat
    /// even when no events are arriving. `main` calls this in a loop.
    pub async fn tick(&mut self) {
        // Sleeping path: drain any pending events first, then halt
        // the CPU in hardware light sleep until a wake source fires.
        if self.model.sleeping {
            while let Ok(event) = EVENTS.try_receive() {
                self.handle_event(event).await;
                if !self.model.sleeping {
                    return;
                }
            }
            self.enter_light_sleep();

            // CPU just woke. The wake-source ISR marked its owning
            // task ready but the task hasn't run yet, so `try_receive`
            // here would race ahead of it. Wait with a short timeout
            // for the first event, then drain any follow-ups. Timer
            // wake with nothing else happening just falls through.
            match select(
                EVENTS.receive(),
                Timer::after(Duration::from_millis(50)),
            ).await {
                Either::First(event) => self.handle_event(event).await,
                Either::Second(_) => {}
            }
            while let Ok(event) = EVENTS.try_receive() {
                self.handle_event(event).await;
            }
            return;
        }

        // Awake path: wait for events with an idle-tick heartbeat so
        // the Model's dim / idle-sleep timers keep advancing even if
        // nothing is happening. 1 s is plenty - thresholds are in
        // multi-second territory.
        const IDLE_TICK: Duration = Duration::from_secs(1);
        match select(EVENTS.receive(), Timer::after(IDLE_TICK)).await {
            Either::First(event) => self.handle_event(event).await,
            Either::Second(_) => {} // idle heartbeat
        }

        // Advance time-driven Model state (buzz phase, dim/idle
        // sleep transitions) and execute the resulting effects.
        let effects = self.model.tick(Instant::now());
        self.execute_effects(effects).await;

        if !self.model.sleeping && self.model.needs_redraw() {
            set_cpu_freq(CpuFreq::Mhz160);
            self.render().await;
            set_cpu_freq(CpuFreq::Mhz80);
        }

        self.log_diagnostics();
        self.tick_count = self.tick_count.wrapping_add(1);
        self.model.cached_data.tick_count = self.tick_count;
    }

    /// Render the active screen with dirty-row flushing. Only runs
    /// when awake and `Model::needs_redraw()` is true.
    async fn render(&mut self) {
        let render_start = Instant::now();
        // Copy the cache so we can freely borrow `&mut self.display`
        // below. `SystemData` is `Copy`, so this is cheap.
        let data = self.model.cached_data;
        self.display.clear(crate::ui::theme::BG).ok();
        self.model.screen.render(&mut self.display, &data);
        if let Some(pct) = data.power.battery_percent {
            primitives::battery_warning_frame(&mut self.display, pct);
        }

        // Hash rows, find dirty band, push only the changed range.
        let fb = self.display.framebuffer();
        let mut min_y: Option<u16> = None;
        let mut max_y: u16 = 0;
        for y in 0..HEIGHT {
            let off = y as usize * ROW_STRIDE;
            let h = row_hash(&fb[off..off + ROW_STRIDE]);
            if h != self.row_hashes[y as usize] {
                self.row_hashes[y as usize] = h;
                if min_y.is_none() { min_y = Some(y); }
                max_y = y;
            }
        }
        if let Some(y0) = min_y {
            // Wait for TE rising edge (vblank) before pushing pixels.
            // Timeout at ~2 refresh periods in case TE is silent.
            let _ = with_timeout(
                Duration::from_millis(30),
                self.lcd_te.wait_for_rising_edge(),
            ).await;
            self.display.flush_rows(y0, max_y + 1).await;
        }
        self.model.clear_redraw();
        let render_ms = render_start.elapsed().as_millis();
        if render_ms > 10 {
            log::info!("render: {}ms", render_ms);
        }
    }

    /// Log heap watermark every ~2000 loop iterations. Useful
    /// for spotting allocation churn in the hot path.
    fn log_diagnostics(&self) {
        if self.tick_count % 2000 == 0 {
            log::info!(
                "heap: used={} free={}",
                esp_alloc::HEAP.used(),
                esp_alloc::HEAP.free(),
            );
        }
    }
}
