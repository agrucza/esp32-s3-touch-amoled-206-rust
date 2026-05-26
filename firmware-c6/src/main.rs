#![no_std]
#![no_main]

extern crate alloc;

mod board;
mod system;

use crate::system::power::C6Board;
use system_core::display::{init_display, Display};
use system_core::flash_fs::FlashRegion;
use system_core::manager::{run, Bringup};
use system_core::storage::Store;
use system_core::tasks::{
    boot_button::BootButtonTaskState,
    imu::ImuTaskState,
    power::PowerTaskState,
    rtc::RtcTaskState,
    touch::TouchTaskState,
};
use embassy_time::{Duration, Timer};
use esp_backtrace as _;
use esp_hal::gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull, WakeEvent};
use esp_hal::i2c::master::{Config as I2cConfig, I2c};
use esp_hal::peripherals as p;
use esp_hal::time::Rate;
use esp_hal::timer::timg::TimerGroup;
use esp_hal::Blocking;

esp_bootloader_esp_idf::esp_app_desc!();

/// C6 boot-construction seam. Mirrors `firmware-s3`'s `S3Bringup` in
/// role; differs only where the hardware does: no SD slot
/// (`init_flash_only`), no TE GPIO (`lcd_te: None`), no RTC_INT GPIO
/// (`RtcTaskState::init(None, ...)`), and an FT3168 wake-from-MONITOR
/// poll in `wait_for_peripherals`.
struct C6Bringup {
    i2c0: Option<p::I2C0<'static>>,
    i2c_sda: Option<p::GPIO8<'static>>,
    i2c_scl: Option<p::GPIO7<'static>>,
    spi2: Option<p::SPI2<'static>>,
    lcd_sclk: Option<p::GPIO0<'static>>,
    lcd_sio0: Option<p::GPIO1<'static>>,
    lcd_sio1: Option<p::GPIO2<'static>>,
    lcd_sio2: Option<p::GPIO3<'static>>,
    lcd_sio3: Option<p::GPIO4<'static>>,
    lcd_cs: Option<p::GPIO5<'static>>,
    dma_ch0: Option<p::DMA_CH0<'static>>,
    lcd_reset: Option<p::GPIO11<'static>>,
    touch_rst: Option<p::GPIO10<'static>>,
    touch_int: Option<p::GPIO15<'static>>,
    btn_boot: Option<p::GPIO9<'static>>,
    imu_int1: Option<p::GPIO16<'static>>,
    flash: Option<p::FLASH<'static>>,
    lpwr: Option<p::LPWR<'static>>,
    // Audio - TX-only speaker path. The mic input (ASDOUT GPIO21) is
    // deliberately left unclaimed for a future capture path.
    i2s0: Option<p::I2S0<'static>>,
    dma_ch1: Option<p::DMA_CH1<'static>>,
    spk_mclk: Option<p::GPIO19<'static>>,
    spk_bclk: Option<p::GPIO20<'static>>,
    spk_ws: Option<p::GPIO22<'static>>,
    spk_dout: Option<p::GPIO23<'static>>,
    spk_pa: Option<p::GPIO6<'static>>,
}

impl Bringup for C6Bringup {
    type Board = C6Board;

    fn make_i2c(&mut self) -> I2c<'static, Blocking> {
        I2c::new(
            self.i2c0.take().unwrap(),
            I2cConfig::default().with_frequency(Rate::from_khz(400)),
        )
        .unwrap()
        .with_sda(self.i2c_sda.take().unwrap())
        .with_scl(self.i2c_scl.take().unwrap())
    }

    fn make_power(
        &mut self,
        i2c: &mut I2c<'static, Blocking>,
    ) -> (Self::Board, PowerTaskState) {
        // No rail config: the AXP retains rail state across resets on
        // this board. C6Board::init only sanity-checks the chip ID.
        let (board, pmu) = C6Board::init(i2c);
        (board, PowerTaskState::new(pmu))
    }

    async fn wait_for_peripherals(&mut self, i2c: &mut I2c<'static, Blocking>) {
        // The FT3168 is left in TOUCH_POWER_MONITOR by prior firmware
        // and doesn't ACK its I2C address for ~2-3 s after boot.
        const TIMEOUT_MS: u32 = 5000;
        const POLL_MS: u32 = 500;
        log::info!(
            "Waiting for FT3168 (0x{:02X}) to wake (timeout {} ms)...",
            board::TOUCH_I2C_ADDR, TIMEOUT_MS,
        );
        let mut elapsed = 0u32;
        let mut awake = false;
        while elapsed < TIMEOUT_MS {
            Timer::after(Duration::from_millis(POLL_MS as u64)).await;
            elapsed += POLL_MS;
            let mut byte = [0u8; 1];
            if i2c.read(board::TOUCH_I2C_ADDR, &mut byte).is_ok() {
                log::info!("  FT3168 awake after {} ms", elapsed);
                awake = true;
                break;
            }
        }
        if !awake {
            log::warn!("  FT3168 did not ACK within {} ms", TIMEOUT_MS);
        }
    }

    async fn make_display(&mut self) -> Display<'static> {
        let fb: &'static mut [u8] = firmware_hal::display::take_framebuffer();
        init_display(
            self.spi2.take().unwrap(),
            self.lcd_sclk.take().unwrap(),
            self.lcd_sio0.take().unwrap(),
            self.lcd_sio1.take().unwrap(),
            self.lcd_sio2.take().unwrap(),
            self.lcd_sio3.take().unwrap(),
            self.lcd_cs.take().unwrap(),
            self.dma_ch0.take().unwrap(),
            Output::new(self.lcd_reset.take().unwrap(), Level::High, OutputConfig::default()),
            fb,
        )
        .await
    }

    /// No TE GPIO on this board - the manager skips the vblank wait
    /// entirely (zero delay, no timeout).
    fn make_lcd_te(&mut self) -> Option<Input<'static>> {
        None
    }

    async fn make_input(
        &mut self,
        i2c: &mut I2c<'static, Blocking>,
    ) -> (TouchTaskState<'static>, BootButtonTaskState<'static>) {
        let mut touch_int = Input::new(
            self.touch_int.take().unwrap(),
            InputConfig::default().with_pull(Pull::Up),
        );
        let mut boot_btn = Input::new(
            self.btn_boot.take().unwrap(),
            InputConfig::default().with_pull(Pull::Up),
        );
        let _ = touch_int.wakeup_enable(true, WakeEvent::LowLevel);
        let _ = boot_btn.wakeup_enable(true, WakeEvent::LowLevel);

        let touch = TouchTaskState::init(
            Output::new(self.touch_rst.take().unwrap(), Level::High, OutputConfig::default()),
            touch_int,
            i2c,
        )
        .await;
        (touch, BootButtonTaskState::new(boot_btn))
    }

    /// Flash only - no SD slot on this board.
    fn make_store(&mut self) -> Store<'static> {
        Store::init_flash_only(
            self.flash.take().unwrap(),
            FlashRegion::new(board::FLASH_FS_START, board::FLASH_FS_SIZE),
        )
    }

    async fn make_sensors(
        &mut self,
        i2c: &mut I2c<'static, Blocking>,
    ) -> (RtcTaskState<'static>, ImuTaskState<'static>) {
        // No RTC_INT GPIO routed on this board -> poll-only RTC task.
        let rtc_state = RtcTaskState::init(None, i2c);
        let imu = ImuTaskState::init(
            Input::new(self.imu_int1.take().unwrap(), InputConfig::default().with_pull(Pull::Down)),
            i2c,
        )
        .await;
        (rtc_state, imu)
    }

    fn make_rtc_ctrl(&mut self) -> esp_hal::rtc_cntl::Rtc<'static> {
        esp_hal::rtc_cntl::Rtc::new(self.lpwr.take().unwrap())
    }

    fn spawn_audio(
        &mut self,
        spawner: embassy_executor::Spawner,
        i2c_bus: &'static system_core::bus::SharedI2c,
    ) {
        // TX circular DMA buffer for the speaker: 4 KB ~= 64 ms at
        // 16 kHz / 16-bit stereo, enough to ride DMA refills without
        // audible gaps. The macro allocates these as internal-SRAM
        // statics (RX side unused -> size 0).
        let (_, _, tx_buffer, tx_descriptors) = esp_hal::dma_circular_buffers!(0, 4096);
        spawner.spawn(
            audio_task(
                i2c_bus,
                self.i2s0.take().unwrap(),
                self.dma_ch1.take().unwrap(),
                self.spk_mclk.take().unwrap(),
                self.spk_bclk.take().unwrap(),
                self.spk_ws.take().unwrap(),
                self.spk_dout.take().unwrap(),
                Output::new(self.spk_pa.take().unwrap(), Level::Low, OutputConfig::default()),
                tx_buffer,
                tx_descriptors,
            )
            .unwrap(),
        );
    }
}

/// Thin per-board wrapper: embassy tasks can't be generic, so each bin
/// monomorphises the shared `run_audio_task` with its concrete I2S /
/// DMA / speaker-pin types here.
#[embassy_executor::task]
async fn audio_task(
    i2c_bus: &'static system_core::bus::SharedI2c,
    i2s: p::I2S0<'static>,
    dma: p::DMA_CH1<'static>,
    mclk: p::GPIO19<'static>,
    bclk: p::GPIO20<'static>,
    ws: p::GPIO22<'static>,
    dout: p::GPIO23<'static>,
    pa: Output<'static>,
    tx_buffer: &'static mut [u8],
    tx_descriptors: &'static mut [esp_hal::dma::DmaDescriptor],
) {
    system_core::audio::run_audio_task(
        i2c_bus, i2s, dma, mclk, bclk, ws, dout, pa, tx_buffer, tx_descriptors,
    )
    .await
}

#[esp_rtos::main]
async fn main(spawner: embassy_executor::Spawner) {
    let peripherals = esp_hal::init(esp_hal::Config::default());

    // No PSRAM on this board: internal-SRAM heap (the framebuffer is
    // in the shared display HAL's static BSS, not the heap).
    esp_alloc::heap_allocator!(size: 64 * 1024);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_int =
        esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_int.software_interrupt0);
    esp_println::logger::init_logger(log::LevelFilter::Info);
    log::info!("--- ESP32-C6-Touch-AMOLED-2.06 booting ---");

    let bringup = C6Bringup {
        i2c0: Some(peripherals.I2C0),
        i2c_sda: Some(peripherals.GPIO8),
        i2c_scl: Some(peripherals.GPIO7),
        spi2: Some(peripherals.SPI2),
        lcd_sclk: Some(peripherals.GPIO0),
        lcd_sio0: Some(peripherals.GPIO1),
        lcd_sio1: Some(peripherals.GPIO2),
        lcd_sio2: Some(peripherals.GPIO3),
        lcd_sio3: Some(peripherals.GPIO4),
        lcd_cs: Some(peripherals.GPIO5),
        dma_ch0: Some(peripherals.DMA_CH0),
        lcd_reset: Some(peripherals.GPIO11),
        touch_rst: Some(peripherals.GPIO10),
        touch_int: Some(peripherals.GPIO15),
        btn_boot: Some(peripherals.GPIO9),
        imu_int1: Some(peripherals.GPIO16),
        flash: Some(peripherals.FLASH),
        lpwr: Some(peripherals.LPWR),
        i2s0: Some(peripherals.I2S0),
        dma_ch1: Some(peripherals.DMA_CH1),
        spk_mclk: Some(peripherals.GPIO19),
        spk_bclk: Some(peripherals.GPIO20),
        spk_ws: Some(peripherals.GPIO22),
        spk_dout: Some(peripherals.GPIO23),
        spk_pa: Some(peripherals.GPIO6),
    };

    run(bringup, spawner).await
}
