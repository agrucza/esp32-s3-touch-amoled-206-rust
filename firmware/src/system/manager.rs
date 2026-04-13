extern crate alloc;

use crate::config::Config;
use crate::display_hal::{self, WIDTH, HEIGHT};
use crate::events::{SwipeDir, SwipeRegion, SystemEvent};
use crate::sdcard_hal::EspVolumeManager;
use crate::system::audio::AudioSystem;
use crate::system::display::{Display, DisplayState};
use crate::system::input::InputSystem;
use crate::system::power::PowerControls;
use crate::system::sensors::SensorSystem;
use crate::system::tasks::imu::MotionData;
use crate::system::tasks::power::PowerTaskState;
use crate::system::tasks::rtc::TimeData;
use crate::system::tasks::touch::TouchData;
use crate::ui::primitives;
use crate::ui::screens::{self, ActiveScreen};
use crate::ui::types::{Action, ScreenId, SystemData};
use embedded_graphics::draw_target::DrawTarget;
use embassy_futures::select::{select3, Either3};
use embassy_time::{Duration, Instant, Timer, with_timeout};
use esp_hal::{
    dma::DmaDescriptor,
    gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull},
    i2c::master::{Config as I2cConfig, I2c},
    i2s::master::asynch::{I2sReadDmaTransferAsync, I2sWriteDmaTransferAsync},
    time::Rate,
};

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

    // Event sources
    pub power: PowerControls<'d>,
    pub power_state: PowerTaskState,
    pub input: InputSystem<'d>,

    // Peripherals
    pub display: Display<'d>,
    pub audio: Option<AudioSystem<'d>>,
    pub sensors: SensorSystem,
    pub storage: Option<EspVolumeManager<'d>>,

    // Tearing-effect line from the display. We wait for its rising
    // edge (vblank start) before pushing pixels so partial flushes
    // don't land mid-scanout.
    lcd_te: Input<'d>,

    // RTC interrupt line (GPIO39, active-low). Shared by the
    // half-minute tick, alarm match, and countdown timer expiry
    // on the PCF85063A. Used as an async wake source so the main
    // loop can sleep longer when the display is off instead of
    // polling the RTC over I2C.
    rtc_int: Input<'d>,

    // IMU INT1 line (GPIO21). Used for Wake-on-Motion when the
    // display is Off. Configured with initial value low; toggles
    // high on WoM event. Reset to low by reading STATUS1.
    imu_int1: Input<'d>,

    // DMA transfers - populated by `start_audio`, `None` until then.
    tx_transfer: Option<AudioTx<'d>>,
    rx_transfer: Option<AudioRx<'d>>,

    // Raw audio peripheral tokens stashed at boot, consumed by
    // `start_audio`. `None` after audio has been started once.
    pending_audio: Option<PendingAudio<'d>>,

    // UI
    screen: ActiveScreen,
    tick_count: u32,
    touch_pos: Option<(u16, u16)>,
    needs_redraw: bool,

    // Pre-allocated drain buffer for the mic RX DMA ring. Lives as a
    // field so tick() never hits the heap.
    mic_drain_buf: alloc::boxed::Box<[u8]>,

    // Per-row FNV-1a hashes from the previous frame. Compared against
    // the current frame to determine which rows actually changed, so
    // only the dirty horizontal band is pushed over QSPI.
    // Initialized to zero, which ensures the first frame sees every
    // row as dirty and performs a full flush.
    row_hashes: alloc::boxed::Box<[u32]>,

    // Runtime configuration (display dim/off timeouts, brightness,
    // ...). Read-only for the current pass, mutable by future settings
    // code. See `crate::config` for the backing types.
    config: Config,

    // Display power-management state. Driven by idle time against
    // `last_activity`; transitions are applied via
    // `system::display::transition`.
    display_state: DisplayState,

    // Wall-clock timestamp of the last user-input event. Used by the
    // display state machine to decide when to dim / blank the panel.
    last_activity: Instant,

    // System-wide sleep flag. When `true`, the display is forced
    // Off and the IMU is in WoM mode regardless of idle time.
    // Set by user request (BOOT button while Active/Dim), cleared
    // by any user-input wake event. Will grow later to also drive
    // CPU frequency scaling and other peripheral power-down.
    sleeping: bool,

    // Event carried over from the async sleep handler (end of tick)
    // to the next tick's event loop. Used to inject WakeOnMotion
    // events so WoM flows through the normal event dispatch path.
    pending_event: Option<SystemEvent>,

    // Cached snapshot of all system data (time, battery, IMU,
    // touch, ...). Event handlers update individual fields as
    // events arrive; the render path just reads this cache.
    // Fully refreshed at boot and on wake-from-sleep.
    cached_data: SystemData,
}

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

    // PSRAM
    pub psram: esp_hal::peripherals::PSRAM<'d>,

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
    /// 3. PSRAM allocator + framebuffer
    /// 4. Display
    /// 5. Input (touch + buttons)
    /// 6. SD card
    /// 7. Sensors (RTC + IMU with ~500ms gyro calibration)
    /// 8. Audio peripherals stashed into `pending_audio` but NOT
    ///    started. A caller invokes `start_audio()` later to actually
    ///    bring the codecs, I2S DMA, and speaker amp online. This
    ///    keeps idle current low on battery by default.
    pub async fn init(p: Peripherals<'static>) -> Self {
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

        // 3. PSRAM allocator + framebuffer
        esp_alloc::psram_allocator!(p.psram, esp_hal::psram);
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

        // 5. Input (buttons + touch)
        let input = InputSystem::init(
            Input::new(p.btn_boot, InputConfig::default().with_pull(Pull::Up)),
            Output::new(p.touch_rst, Level::High, OutputConfig::default()),
            Input::new(p.touch_int, InputConfig::default().with_pull(Pull::Up)),
            &mut i2c,
        ).await;

        // RTC interrupt line (GPIO39). The PCF85063A pulses this
        // low on any of: half-minute tick (second=0 / second=30),
        // alarm match, or countdown timer expiry. Used as an async
        // wake source so the display-off sleep can last seconds
        // instead of 150 ms.
        let rtc_int = Input::new(p.rtc_int, InputConfig::default().with_pull(Pull::Up));

        // IMU INT1 line. Used for Wake-on-Motion when display is Off.
        // No pull - the QMI8658C drives the line from configured
        // initial value to its opposite on each WoM event.
        let imu_int1 = Input::new(p.imu_int1, InputConfig::default().with_pull(Pull::None));

        // 6. SD card
        let storage = crate::system::storage::init_sd(
            p.spi3, p.sd_sck, p.sd_mosi, p.sd_miso,
            Output::new(p.sd_cs, Level::High, OutputConfig::default()),
        );

        // 7. Sensors (RTC + IMU with gyro calibration)
        let sensors = SensorSystem::init(&mut i2c).await;

        // 7b. Enable the RTC half-minute interrupt so GPIO39 pulses
        // low at second=0 and second=30 of every minute.
        if let Err(_) = sensors.rtc.enable_half_minute_interrupt(&mut i2c) {
            log::warn!("RTC: failed to enable half-minute interrupt");
        }

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

        Self {
            i2c_bus,
            power,
            power_state,
            input,
            display,
            audio: None,
            sensors,
            storage,
            lcd_te,
            rtc_int,
            imu_int1,
            tx_transfer: None,
            rx_transfer: None,
            pending_audio,
            screen: ActiveScreen::new(ScreenId::Clock),
            tick_count: 0,
            touch_pos: None,
            needs_redraw: true, // first frame always draws
            mic_drain_buf: alloc::vec![0u8; MIC_DRAIN_BUF_SIZE].into_boxed_slice(),
            row_hashes: alloc::vec![0u32; HEIGHT as usize].into_boxed_slice(),
            config: Config::default(),
            display_state: DisplayState::Active,
            last_activity: Instant::now(),
            sleeping: false,
            pending_event: None,
            cached_data: SystemData::default(),
        }
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

    // ================= Sleep / wake API ==========================================

    /// Returns `true` if the event should count as user activity
    /// (resets idle timer; wakes from sleep).
    fn is_user_activity(event: &SystemEvent) -> bool {
        matches!(
            event,
            SystemEvent::TouchPressed { .. }
                | SystemEvent::TouchReleased
                | SystemEvent::Tap { .. }
                | SystemEvent::Swipe { .. }
                | SystemEvent::BootButtonPressed
                | SystemEvent::PowerButtonShort
        )
    }

    /// Enter low-power sleep. Idempotent.
    ///
    /// Puts the IMU into Wake-on-Motion mode, blanks the display,
    /// and sets the `sleeping` flag. Peripheral power-down for
    /// other subsystems (audio, CPU freq, etc.) will hang off
    /// this method in the future.
    async fn sleep(&mut self) {
        if self.sleeping {
            return;
        }
        log::info!("system: sleep");
        self.sleeping = true;
        {
            let mut i2c = self.i2c_bus.lock().await;
            self.sensors.enter_wom_mode(&mut *i2c);
        }
        let _ = crate::system::display::transition(
            &mut self.display,
            self.display_state,
            DisplayState::Off,
            &self.config.display,
        ).await;
        self.display_state = DisplayState::Off;
    }

    /// Exit low-power sleep. Idempotent.
    ///
    /// Restores the IMU to normal mode, turns the display back on
    /// at full brightness, resets the idle timer, and forces a
    /// full redraw so the first frame after wake is current.
    async fn wake(&mut self) {
        if !self.sleeping {
            return;
        }
        log::info!("system: wake");
        self.sleeping = false;
        self.last_activity = Instant::now();
        let _ = crate::system::display::transition(
            &mut self.display,
            self.display_state,
            DisplayState::Active,
            &self.config.display,
        ).await;
        self.display_state = DisplayState::Active;
        {
            let mut i2c = self.i2c_bus.lock().await;
            self.sensors.exit_wom_mode(&mut *i2c);
        }
        self.row_hashes.fill(0);
        self.needs_redraw = true;
    }

    /// Apply the Active <-> Dim transition when awake. No-op when
    /// sleeping (the display is Off and `sleep`/`wake` handle that).
    async fn apply_dim_state(&mut self) {
        if self.sleeping {
            return;
        }
        let idle = Instant::now().duration_since(self.last_activity);
        let target = if idle >= Duration::from_secs(self.config.display.dim_timeout_s) {
            DisplayState::Dim
        } else {
            DisplayState::Active
        };
        if target != self.display_state {
            let _ = crate::system::display::transition(
                &mut self.display,
                self.display_state,
                target,
                &self.config.display,
            ).await;
            self.display_state = target;
        }
    }

    /// Trigger sleep if the idle timer has expired. No-op if
    /// already sleeping.
    async fn check_idle_sleep(&mut self) {
        if self.sleeping {
            return;
        }
        let idle = Instant::now().duration_since(self.last_activity);
        if idle >= Duration::from_secs(self.config.display.off_timeout_s) {
            self.sleep().await;
        }
    }

    // ================= Tick helpers ==============================================

    /// Drain the mic DMA buffer (no-op until audio is started).
    async fn drain_audio(&mut self) {
        if let Some(rx) = self.rx_transfer.as_mut() {
            let _ = rx.pop(&mut self.mic_drain_buf).await;
        }
    }

    /// Poll raw events from all subsystems, plus any pending event
    /// injected by the previous tick's sleep handler (e.g. WoM).
    async fn poll_events(&mut self) -> heapless::Vec<SystemEvent, 8> {
        let mut events: heapless::Vec<SystemEvent, 8> = heapless::Vec::new();
        {
            let mut i2c = self.i2c_bus.lock().await;
            self.input.poll(&mut *i2c, &mut events);
            self.power_state.poll(&mut *i2c, &mut events);
            if !self.sleeping {
                self.sensors.poll(&mut *i2c, &mut events);
            }
        }
        if let Some(e) = self.pending_event.take() {
            let _ = events.push(e);
        }
        events
    }

    /// Collect a fresh `SystemData` snapshot from all subsystems.
    async fn read_snapshot(&mut self) -> SystemData {
        let mut i2c = self.i2c_bus.lock().await;
        let sensors = self.sensors.snapshot(&mut *i2c);
        let power = self.power_state.snapshot(&mut *i2c);
        drop(i2c);

        // Convert driver-level types into the UI-facing grouped
        // data structs. `From` impls live next to each task's
        // data type in `system::tasks::*`.
        let time = sensors.time.as_ref().map(TimeData::from).unwrap_or_default();
        let motion = sensors.imu.as_ref().map(MotionData::from).unwrap_or_default();
        let touch = TouchData {
            x: self.touch_pos.map(|(x, _)| x),
            y: self.touch_pos.map(|(_, y)| y),
        };

        SystemData {
            time,
            power,
            motion,
            touch,
            tick_count: self.tick_count,
        }
    }

    /// Build an empty `SystemData` for ticks where we skip sensor
    /// reads (sleeping with no wake pending). Touch position is
    /// preserved from the cache since it's local state.
    fn empty_snapshot(&self) -> SystemData {
        SystemData {
            time: TimeData::default(),
            power: crate::system::tasks::power::PowerData::default(),
            motion: MotionData::default(),
            touch: TouchData {
                x: self.touch_pos.map(|(x, _)| x),
                y: self.touch_pos.map(|(_, y)| y),
            },
            tick_count: self.tick_count,
        }
    }

    /// Process events: sleep/wake transitions, touch tracking,
    /// redraw triggers, haptic feedback, and screen dispatch.
    async fn handle_events(&mut self, events: &[SystemEvent], data: &SystemData) {
        for event in events {
            // Track touch drag position and redraw triggers regardless
            // of sleep state (cheap and safe).
            match event {
                SystemEvent::TouchPressed { x, y } => self.touch_pos = Some((*x, *y)),
                SystemEvent::TouchReleased => self.touch_pos = None,
                SystemEvent::HalfMinuteChanged
                | SystemEvent::BatteryChanged { .. } => self.needs_redraw = true,
                _ => {}
            }

            // WoM wake: clear sleep flag and skip screen dispatch.
            if matches!(event, SystemEvent::WakeOnMotion) {
                log::info!("wake: IMU motion");
                self.wake().await;
                continue;
            }

            // Sleep/wake transitions on user activity.
            if Self::is_user_activity(event) {
                self.last_activity = Instant::now();
                if self.sleeping {
                    // Waking from sleep: consume this event so it
                    // doesn't dispatch to the screen (avoids
                    // accidental taps/swipes on wake).
                    self.wake().await;
                    continue;
                }
                if matches!(event, SystemEvent::BootButtonPressed) {
                    // BOOT while awake = "sleep now" shortcut.
                    // Haptic feedback fires below before we go to sleep.
                    self.power.buzz();
                    Timer::after(Duration::from_millis(100)).await;
                    self.power.buzz_stop();
                    self.sleep().await;
                    continue;
                }
            }

            // From here on we only dispatch to the screen when awake.
            if self.sleeping {
                continue;
            }

            // Log swipes for debugging.
            if let SystemEvent::Swipe { dir, region } = event {
                log::info!("Swipe: {:?} in {:?}", dir, region);
            }

            // System-level gesture: swipe-down-from-top opens panel.
            if !matches!(self.screen.id(), ScreenId::Panel) {
                if let SystemEvent::Swipe { dir: SwipeDir::Down, region: SwipeRegion::Top } = event {
                    let previous = self.screen.id();
                    self.screen = ActiveScreen::new_panel(previous);
                    self.needs_redraw = true;
                    continue;
                }
            }

            // Haptic feedback for BOOT presses that reached this
            // branch (currently unreachable since BOOT short-circuits
            // above, but kept for future screens that want to handle
            // BOOT explicitly).

            // Forward to the active screen.
            match self.screen.on_event(event, data) {
                Action::None => {
                    // Home-row nav fallback: content L/R swipes cycle
                    // through home-row apps.
                    if !matches!(self.screen.id(), ScreenId::Panel) {
                        if let SystemEvent::Swipe { dir, region: SwipeRegion::Content } = event {
                            match dir {
                                SwipeDir::Right => {
                                    let next = screens::cycle_home_app(self.screen.id(), true);
                                    self.screen.switch_to(next);
                                    self.needs_redraw = true;
                                }
                                SwipeDir::Left => {
                                    let prev = screens::cycle_home_app(self.screen.id(), false);
                                    self.screen.switch_to(prev);
                                    self.needs_redraw = true;
                                }
                                _ => {}
                            }
                        }
                    }
                }
                Action::Redraw => self.needs_redraw = true,
                Action::SwitchScreen(id) => {
                    self.screen.switch_to(id);
                    self.needs_redraw = true;
                }
                Action::Shutdown => {
                    log::info!("System: shutdown requested");
                    self.power.shutdown();
                }
            }
        }
    }

    /// Render the active screen with dirty-row flushing. Only runs
    /// when awake and `needs_redraw` is set.
    async fn render(&mut self, data: &SystemData) {
        self.display.clear(crate::ui::theme::BG).ok();
        self.screen.render(&mut self.display, data);
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
        self.needs_redraw = false;
    }

    /// Async wait for the next event. Adaptive sleep behavior:
    ///
    /// - **Sleeping**: 2 s timeout, waiting on touch/BOOT INT, RTC
    ///   INT (half-minute tick / alarm / timer), or IMU WoM INT.
    ///   WoM wakes inject a `WakeOnMotion` event for the next
    ///   tick's event loop.
    /// - **Awake, finger down**: 50 ms timeout for tight drag tracking.
    /// - **Awake, idle**: 150 ms timeout for sensor polling.
    async fn wait_for_next_event(&mut self) {
        if self.sleeping {
            let result = with_timeout(
                Duration::from_secs(2),
                select3(
                    self.input.wait_for_user_input(),
                    self.rtc_int.wait_for_falling_edge(),
                    self.imu_int1.wait_for_rising_edge(),
                ),
            ).await;
            match result {
                Ok(Either3::First(_)) => {}  // touch/BOOT - handled via event poll
                Ok(Either3::Second(_)) => {} // RTC minute - sensors.poll would pick it up but we skip when sleeping
                Ok(Either3::Third(_)) => {
                    // WoM: clear STATUS1 flag and inject event.
                    let mut i2c = self.i2c_bus.lock().await;
                    let _ = self.sensors.imu.wom_event(&mut *i2c);
                    drop(i2c);
                    let _ = self.pending_event.insert(SystemEvent::WakeOnMotion);
                }
                Err(_) => {
                    // Timeout: check if WoM fired but we missed the edge.
                    let mut i2c = self.i2c_bus.lock().await;
                    let fired = self.sensors.imu.wom_event(&mut *i2c).unwrap_or(false);
                    drop(i2c);
                    if fired {
                        let _ = self.pending_event.insert(SystemEvent::WakeOnMotion);
                    }
                }
            }
        } else {
            let timeout = if self.touch_pos.is_some() {
                Duration::from_millis(50)
            } else {
                Duration::from_millis(150)
            };
            let _ = with_timeout(timeout, self.input.wait_for_user_input()).await;
        }
    }

    /// Log heap watermark every ~10 s (200 ticks * 50 ms). Useful
    /// for spotting allocation churn in the hot path.
    fn log_diagnostics(&self) {
        if self.tick_count % 200 == 0 {
            log::info!(
                "heap: used={} free={}",
                esp_alloc::HEAP.used(),
                esp_alloc::HEAP.free(),
            );
        }
    }

    // ================= Tick (orchestration) ======================================

    /// Run one iteration of the main loop.
    pub async fn tick(&mut self) {
        self.drain_audio().await;

        let events = self.poll_events().await;

        // Predict whether we're about to wake so we read fresh
        // sensor data now (before handle_events calls wake()) -
        // otherwise the first frame after wake would render stale
        // or empty data.
        let will_wake = self.sleeping && events.iter().any(|e| {
            Self::is_user_activity(e) || matches!(e, SystemEvent::WakeOnMotion)
        });
        let data = if !self.sleeping || will_wake {
            self.read_snapshot().await
        } else {
            self.empty_snapshot()
        };

        self.handle_events(&events, &data).await;
        self.check_idle_sleep().await;
        self.apply_dim_state().await;

        if !self.sleeping && self.needs_redraw {
            self.render(&data).await;
        }

        self.log_diagnostics();
        self.tick_count = self.tick_count.wrapping_add(1);

        self.wait_for_next_event().await;
    }
}
