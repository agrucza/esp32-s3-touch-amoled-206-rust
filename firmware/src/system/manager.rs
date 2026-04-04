extern crate alloc;

use crate::display_hal::{self, CO5300, EspQspi};
use crate::events::SystemEvent;
use crate::sdcard_hal::EspVolumeManager;
use crate::system::{audio::AudioSystem, input::InputSystem, power::PowerSystem, sensors::SensorSystem};
use embassy_time::{Duration, Timer};
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

    // DMA transfers
    tx_transfer: AudioTx<'d>,
    rx_transfer: AudioRx<'d>,
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
            tx_transfer,
            rx_transfer,
        }
    }

    /// Run one iteration of the main loop.
    ///
    /// Drains the audio DMA buffer, polls all event sources,
    /// dispatches events, then sleeps for the tick interval.
    pub async fn tick(&mut self) {
        // Drain mic RX buffer to prevent overflow
        {
            let mut pcm = alloc::vec![0u8; 16384];
            let _ = self.rx_transfer.pop(&mut pcm).await;
        }

        // Poll phase - collect events from all subsystems
        let mut events: heapless::Vec<SystemEvent, 8> = heapless::Vec::new();
        self.input.poll(&mut self.i2c, &mut events);
        self.power.poll(&mut self.i2c, &mut events);

        // Dispatch phase
        for event in events.iter() {
            self.handle_event(event).await;
        }

        Timer::after(Duration::from_millis(10)).await;
    }

    /// Handle a single system event.
    async fn handle_event(&mut self, event: &SystemEvent) {
        match event {
            SystemEvent::BootButtonPressed => {
                log::info!("BTN: BOOT pressed");
                self.power.buzz();
                Timer::after(Duration::from_millis(200)).await;
                self.power.buzz_stop();
            }
            SystemEvent::PowerButtonShort => {
                log::info!("BTN: PWR short press");
            }
            SystemEvent::PowerButtonLong => {
                log::info!("BTN: PWR long press");
            }
            SystemEvent::TouchPressed { x, y } => {
                log::info!("Touch: ({}, {})", x, y);
            }
            SystemEvent::TouchReleased => {
                log::info!("Touch: released");
            }
        }
    }
}
