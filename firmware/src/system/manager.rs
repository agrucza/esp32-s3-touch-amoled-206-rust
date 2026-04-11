extern crate alloc;

use crate::display_hal::{self, CO5300, EspQspi, WIDTH, HEIGHT};
use crate::events::{SwipeDir, SwipeRegion, SystemEvent};
use crate::sdcard_hal::EspVolumeManager;
use crate::system::{audio::AudioSystem, input::InputSystem, power::PowerSystem, sensors::SensorSystem};
use crate::ui::primitives;
use crate::ui::screens::{self, ActiveScreen};
use crate::ui::types::{Action, ScreenId, SystemData};
use embedded_graphics::draw_target::DrawTarget;
use embassy_time::{Duration, Timer, with_timeout};
use esp_hal::{
    Blocking,
    dma::DmaDescriptor,
    gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull},
    i2c::master::{Config as I2cConfig, I2c},
    i2s::master::asynch::{I2sReadDmaTransferAsync, I2sWriteDmaTransferAsync},
    time::Rate,
};

// Type aliases for complex generic types
pub type Display<'d> = CO5300<'static, EspQspi<'d>, Output<'d>>;
pub type AudioTx<'d> = I2sWriteDmaTransferAsync<'d, &'static mut [u8]>;
pub type AudioRx<'d> = I2sReadDmaTransferAsync<'d, &'static mut [u8]>;

/// Size of the per-tick mic RX drain buffer. Matches the I2S RX DMA
/// buffer in `main.rs` so one `pop()` can empty a full buffer.
const MIC_DRAIN_BUF_SIZE: usize = 16_384;

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

// `audio`, `storage`, and `tx_transfer` are currently held-but-not-read:
// they own hardware resources whose Drop impls would deinitialize the
// peripherals. They stay as fields so the hardware keeps running.
#[allow(dead_code)]
pub struct SystemManager<'d> {
    // Bus
    i2c: I2c<'d, Blocking>,

    // Event sources
    pub power: PowerSystem<'d>,
    pub input: InputSystem<'d>,

    // Peripherals
    pub display: Display<'d>,
    pub audio: AudioSystem<'d>,
    pub sensors: SensorSystem,
    pub storage: Option<EspVolumeManager<'d>>,

    // Tearing-effect line from the display. We wait for its rising
    // edge (vblank start) before pushing pixels so partial flushes
    // don't land mid-scanout.
    lcd_te: Input<'d>,

    // DMA transfers
    tx_transfer: AudioTx<'d>,
    rx_transfer: AudioRx<'d>,

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
    /// 8. Audio (I2S DMA must start before codec init)
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

        // 6. SD card
        let storage = crate::system::storage::init_sd(
            p.spi3, p.sd_sck, p.sd_mosi, p.sd_miso,
            Output::new(p.sd_cs, Level::High, OutputConfig::default()),
        );

        // 7. Sensors (RTC + IMU with gyro calibration)
        let sensors = SensorSystem::init(&mut i2c).await;

        // 8. Audio (I2S + codecs + DMA)
        let (audio, tx_transfer, rx_transfer) = crate::system::audio::init_audio(
            p.i2s0, p.dma_ch1,
            p.audio_mclk, p.audio_bclk, p.audio_ws,
            p.audio_dout, p.audio_din,
            Output::new(p.audio_pa, Level::Low, OutputConfig::default()),
            p.tx_buffer, p.rx_buffer, p.tx_descriptors, p.rx_descriptors,
            &mut i2c,
        ).await;

        log::info!("System: all subsystems initialized");

        Self {
            i2c,
            power,
            input,
            display,
            audio,
            sensors,
            storage,
            lcd_te,
            tx_transfer,
            rx_transfer,
            screen: ActiveScreen::new(ScreenId::Clock),
            tick_count: 0,
            touch_pos: None,
            needs_redraw: true, // first frame always draws
            mic_drain_buf: alloc::vec![0u8; MIC_DRAIN_BUF_SIZE].into_boxed_slice(),
            row_hashes: alloc::vec![0u32; HEIGHT as usize].into_boxed_slice(),
        }
    }

    /// Run one iteration of the main loop.
    ///
    /// Drains audio DMA, polls events, routes them through the
    /// active screen. Only redraws the display when something changed.
    pub async fn tick(&mut self) {
        // Drain mic RX buffer to prevent overflow. The buffer is a
        // field so this never allocates.
        let _ = self.rx_transfer.pop(&mut self.mic_drain_buf).await;

        // Poll all subsystems for events
        let mut events: heapless::Vec<SystemEvent, 8> = heapless::Vec::new();
        self.input.poll(&mut self.i2c, &mut events);
        self.power.poll(&mut self.i2c, &mut events);
        self.sensors.poll(&mut self.i2c, &mut events);

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

        // Build data snapshot for rendering
        let time = self.sensors.read_time(&mut self.i2c);
        let imu = self.sensors.imu.read(&mut self.i2c).ok();
        let battery = self.power.battery_percent(&mut self.i2c);
        let battery_mv = self.power.battery_voltage_mv(&mut self.i2c);
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

        // Only redraw when something changed
        if self.needs_redraw {
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

        // Adaptive sleep:
        //   - While a finger is down, keep the 50 ms cadence so the
        //     drag stream stays smooth.
        //   - Idle: wait up to 150 ms on the touch INT line. The mic
        //     I2S DMA ring holds ~256 ms at 16 kHz stereo; 150 ms
        //     leaves a comfortable margin before overflow while still
        //     waking instantly on the next touch.
        if self.touch_pos.is_some() {
            Timer::after(Duration::from_millis(50)).await;
        } else {
            let _ = with_timeout(
                Duration::from_millis(150),
                self.input.wait_for_touch_int(),
            ).await;
        }
    }
}
