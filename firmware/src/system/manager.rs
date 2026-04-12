extern crate alloc;

use crate::config::Config;
use crate::display_hal::{self, WIDTH, HEIGHT};
use crate::events::{SwipeDir, SwipeRegion, SystemEvent};
use crate::sdcard_hal::EspVolumeManager;
use crate::system::audio::AudioSystem;
use crate::system::display::{Display, DisplayState};
use crate::system::input::InputSystem;
use crate::system::power::PowerSystem;
use crate::system::sensors::SensorSystem;
use crate::ui::primitives;
use crate::ui::screens::{self, ActiveScreen};
use crate::ui::types::{Action, ScreenId, SystemData};
use embedded_graphics::draw_target::DrawTarget;
use embassy_futures::select::select;
use embassy_time::{Duration, Instant, Timer, with_timeout};
use esp_hal::{
    Blocking,
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
    // Bus
    i2c: I2c<'d, Blocking>,

    // Event sources
    pub power: PowerSystem<'d>,
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

    // RTC minute-interrupt line (GPIO39, active-low pulse). The
    // PCF85063A pulses this at second=0 every minute. Used as an
    // async wake source so the main loop can sleep longer when the
    // display is off instead of polling the RTC over I2C.
    rtc_int: Input<'d>,

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

impl<'d> SystemManager<'d> {
    /// Initialize all subsystems and assemble the system manager.
    ///
    /// This is the single entry point for the entire system. Call it from
    /// main() after HAL init, timer start, and logger setup.
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
    pub async fn init(p: Peripherals<'d>) -> Self {
        // 1. I2C bus
        let mut i2c = I2c::new(p.i2c0, I2cConfig::default().with_frequency(Rate::from_khz(400)))
            .unwrap()
            .with_sda(p.i2c_sda)
            .with_scl(p.i2c_scl);

        // 2. Power - must init first (enables all power rails)
        let power = PowerSystem::init(
            Output::new(p.sys_out_pin, Level::Low, OutputConfig::default()),
            Output::new(p.motor_pin, Level::Low, OutputConfig::default()),
            &mut i2c,
        ).expect("PMU init failed - halting");
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

        // RTC minute-interrupt line. The PCF85063A pulses INT# low at
        // second=0 every minute. We use this as an async wake source
        // so the display-off sleep can last seconds instead of 150 ms.
        let rtc_int = Input::new(p.rtc_int, InputConfig::default().with_pull(Pull::Up));

        // 6. SD card
        let storage = crate::system::storage::init_sd(
            p.spi3, p.sd_sck, p.sd_mosi, p.sd_miso,
            Output::new(p.sd_cs, Level::High, OutputConfig::default()),
        );

        // 7. Sensors (RTC + IMU with gyro calibration)
        let sensors = SensorSystem::init(&mut i2c).await;

        // 7b. Enable the RTC minute interrupt so GPIO39 pulses at second=0.
        if let Err(_) = sensors.rtc.enable_minute_interrupt(&mut i2c) {
            log::warn!("RTC: failed to enable minute interrupt");
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

        Self {
            i2c,
            power,
            input,
            display,
            audio: None,
            sensors,
            storage,
            lcd_te,
            rtc_int,
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
        let (audio, tx_transfer, rx_transfer) = crate::system::audio::init_audio(
            pa.i2s0, pa.dma_ch1,
            pa.audio_mclk, pa.audio_bclk, pa.audio_ws,
            pa.audio_dout, pa.audio_din,
            Output::new(pa.audio_pa, Level::Low, OutputConfig::default()),
            pa.tx_buffer, pa.rx_buffer, pa.tx_descriptors, pa.rx_descriptors,
            &mut self.i2c,
        ).await;
        self.audio = Some(audio);
        self.tx_transfer = Some(tx_transfer);
        self.rx_transfer = Some(rx_transfer);
    }

    /// Run one iteration of the main loop.
    ///
    /// Drains audio DMA, polls events, routes them through the
    /// active screen. Only redraws the display when something changed.
    pub async fn tick(&mut self) {
        // Drain mic RX buffer to prevent overflow, but only when
        // audio has actually been started. The buffer is a field so
        // this never allocates.
        if let Some(rx) = self.rx_transfer.as_mut() {
            let _ = rx.pop(&mut self.mic_drain_buf).await;
        }

        // Poll all subsystems for events. Input and power are always
        // polled (touch wake, power button). Sensor polling (RTC
        // minute-change) is skipped when the display is Off because
        // the RTC minute-interrupt on GPIO39 wakes us instead, and
        // last_minute will re-sync on the first Active tick.
        let mut events: heapless::Vec<SystemEvent, 8> = heapless::Vec::new();
        self.input.poll(&mut self.i2c, &mut events);
        self.power.poll(&mut self.i2c, &mut events);
        if self.display_state != DisplayState::Off {
            self.sensors.poll(&mut self.i2c, &mut events);
        }

        // Track touch state from events, and mark state-change events
        // as redraw triggers. Everything else goes through the screen's
        // Action return value so screens opt in to redraws explicitly
        // (avoids a redraw on every TouchPressed tick during a drag).
        for event in events.iter() {
            match event {
                SystemEvent::TouchPressed { x, y } => self.touch_pos = Some((*x, *y)),
                SystemEvent::TouchReleased => self.touch_pos = None,
                SystemEvent::MinuteChanged
                | SystemEvent::BatteryChanged { .. } => self.needs_redraw = true,
                _ => {}
            }
        }

        // Build data snapshot for rendering. When the display is Off
        // nothing renders, so skip the IMU read and battery I2C
        // transactions entirely - they would just burn bus cycles for
        // data nobody looks at.
        let (time, imu, battery, battery_mv) = if self.display_state != DisplayState::Off {
            (
                self.sensors.read_time(&mut self.i2c),
                self.sensors.imu.read(&mut self.i2c).ok(),
                self.power.battery_percent(&mut self.i2c),
                self.power.battery_voltage_mv(&mut self.i2c),
            )
        } else {
            (None, None, None, None)
        };
        let data = SystemData::from_sensors(
            time.as_ref(),
            imu.as_ref(),
            self.touch_pos,
            battery,
            battery_mv,
            self.tick_count,
        );

        for event in events.iter() {
            // Log every swipe so we can verify detection across all regions.
            if let SystemEvent::Swipe { dir, region } = event {
                log::info!("Swipe: {:?} in {:?}", dir, region);
            }

            // Any user-input event resets the display idle timer. If
            // the display is currently Off, we also "consume" the
            // event - the first touch wakes the panel but does NOT
            // dispatch to the screen, so a brush against clothing
            // while worn on a wrist doesn't accidentally swipe or tap.
            // Dim state still passes events through normally.
            let is_user_activity = matches!(
                event,
                SystemEvent::TouchPressed { .. }
                    | SystemEvent::TouchReleased
                    | SystemEvent::Tap { .. }
                    | SystemEvent::Swipe { .. }
                    | SystemEvent::BootButtonPressed
                    | SystemEvent::PowerButtonShort
            );
            if is_user_activity {
                self.last_activity = Instant::now();
                if self.display_state == DisplayState::Off {
                    continue;
                }
            }

            // System-level gesture: swipe-down-from-top-edge opens
            // the panel screen. Only triggers when we're NOT already
            // in the panel (otherwise the panel screen handles its
            // own swipes). Panel remembers the current screen so it
            // can return to it on close.
            if !matches!(self.screen.id(), ScreenId::Panel) {
                if let SystemEvent::Swipe { dir: SwipeDir::Down, region: SwipeRegion::Top } = event {
                    let previous = self.screen.id();
                    self.screen = ActiveScreen::new_panel(previous);
                    self.needs_redraw = true;
                    continue;
                }
            }

            // Forward to the active screen first. If the screen returns
            // Action::None and the event is a content L/R swipe, fall
            // through to the home-row nav fallback below. This lets a
            // screen (e.g. a horizontal slider) own its L/R gestures by
            // returning anything other than None.
            match self.screen.on_event(event, &data) {
                Action::None => {
                    // Home-row nav fallback: content L/R swipes cycle
                    // through the home-row apps. Panel is modal and
                    // never participates in the carousel, so we skip
                    // the fallback when it's active.
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

        // Haptic feedback on boot button (system-level, not screen-specific)
        if events.iter().any(|e| matches!(e, SystemEvent::BootButtonPressed)) {
            self.power.buzz();
            Timer::after(Duration::from_millis(100)).await;
            self.power.buzz_stop();
        }

        // Apply the display state machine. Decide the target state
        // based on idle time since last user activity and transition
        // if it changed. Waking from Off forces a full redraw by
        // zeroing row_hashes so every row looks dirty on the next
        // pass.
        let idle = Instant::now().duration_since(self.last_activity);
        let target = if idle >= Duration::from_secs(self.config.display.off_timeout_s) {
            DisplayState::Off
        } else if idle >= Duration::from_secs(self.config.display.dim_timeout_s) {
            DisplayState::Dim
        } else {
            DisplayState::Active
        };
        if target != self.display_state {
            let waking_from_off = crate::system::display::transition(
                &mut self.display,
                self.display_state,
                target,
                &self.config.display,
            ).await;
            self.display_state = target;
            if waking_from_off {
                self.row_hashes.fill(0);
                self.needs_redraw = true;
            }
        }

        // Only redraw when something changed AND the display is not
        // in the Off state. When Off, the render path is skipped
        // entirely - the panel's GRAM retains the last frame we
        // pushed so there's nothing to do until a user event wakes
        // us and we come back around.
        if self.display_state != DisplayState::Off && self.needs_redraw {
            self.display.clear(crate::ui::theme::BG).ok();

            // Panel is just another screen now - no special case.
            self.screen.render(&mut self.display, &data);

            // System-wide low-battery warning: a 1-px colored frame
            // tracing the physical display edge. Drawn last so it
            // remains visible over everything.
            if let Some(pct) = data.battery_percent {
                primitives::battery_warning_frame(&mut self.display, pct);
            }

            // Hash each row, compare to the previous frame, push only
            // the contiguous vertical band that changed. If nothing
            // changed we skip the QSPI push entirely.
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
                // Wait for the next vblank (TE rising edge) before
                // pushing pixels, so the flush lands between frames
                // and doesn't tear across scanlines. Timeout at
                // ~2 refresh periods in case TE is silent for any
                // reason - we'd rather flush late than hang.
                let _ = with_timeout(
                    Duration::from_millis(30),
                    self.lcd_te.wait_for_rising_edge(),
                ).await;
                self.display.flush_rows(y0, max_y + 1).await;
            }
            self.needs_redraw = false;
        }

        // Heap watermark log every ~10 s (200 ticks * 50 ms). Used to
        // spot allocation churn in the hot path - `used` should be flat
        // in steady state.
        if self.tick_count % 200 == 0 {
            log::info!(
                "heap: used={} free={}",
                esp_alloc::HEAP.used(),
                esp_alloc::HEAP.free(),
            );
        }

        self.tick_count = self.tick_count.wrapping_add(1);

        // Adaptive sleep - how long and what we wait on depends on
        // the display state:
        //
        // Active / Dim with finger down (50 ms, touch INT only):
        //   Tight drag tracking. The FT3168 asserts INT on every new
        //   sample (60-120 Hz). Timeout covers missed edges.
        //
        // Active / Dim idle (150 ms, touch INT only):
        //   Bounds mic DMA drain (ring holds ~256 ms at 16 kHz
        //   stereo) and sensor/battery polling.
        //
        // Off (2 s, touch INT OR RTC minute INT):
        //   Nothing renders, so the only reasons to wake are a touch
        //   (user wants to interact) or the RTC minute pulse on
        //   GPIO39 (keeps last_minute in sync for the next Active
        //   transition). The 2 s timeout is a safety net for PMU
        //   interrupt polling (power button short/long press is
        //   readable via I2C even without a dedicated GPIO).
        if self.display_state == DisplayState::Off {
            let _ = with_timeout(
                Duration::from_secs(2),
                select(
                    self.input.wait_for_touch_int(),
                    self.rtc_int.wait_for_falling_edge(),
                ),
            ).await;
        } else {
            let sleep = if self.touch_pos.is_some() {
                Duration::from_millis(50)
            } else {
                Duration::from_millis(150)
            };
            let _ = with_timeout(sleep, self.input.wait_for_touch_int()).await;
        }
    }
}
