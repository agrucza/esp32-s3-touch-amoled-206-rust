#![no_std]
#![no_main]

extern crate alloc;

mod board;
// The audio stack (ES8311/ES7210/I2S) lives in `system_core::audio`
// - same hardware on every board. Implemented but not wired into the
// manager loop yet (idle current); bring-up is reintroduced bin-side
// when there's a caller.
mod system;

// `config`/`events`/`ui` live in `app-core` (host-testable);
// re-export at the crate root so existing `crate::config::...` paths
// keep working.
pub use app_core::{config, events, ui};

use crate::system::power::PowerControls;
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
use esp_backtrace as _;
use esp_hal::gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull, WakeEvent};
use esp_hal::i2c::master::{Config as I2cConfig, I2c};
use esp_hal::peripherals as p;
use esp_hal::time::Rate;
use esp_hal::timer::timg::TimerGroup;
use esp_hal::Blocking;

esp_bootloader_esp_idf::esp_app_desc!();

/// S3 boot-construction seam. Holds the raw peripheral tokens (esp-hal
/// singletons can't be partial-moved through `&mut self`, so each is
/// an `Option` `.take()`-n once by its `Bringup` method). The shared
/// `system_core::manager::run` drives the canonical sequence.
struct S3Bringup {
    i2c0: Option<p::I2C0<'static>>,
    i2c_sda: Option<p::GPIO15<'static>>,
    i2c_scl: Option<p::GPIO14<'static>>,
    sys_out: Option<p::GPIO10<'static>>,
    motor: Option<p::GPIO18<'static>>,
    spi2: Option<p::SPI2<'static>>,
    lcd_sclk: Option<p::GPIO11<'static>>,
    lcd_sio0: Option<p::GPIO4<'static>>,
    lcd_sio1: Option<p::GPIO5<'static>>,
    lcd_sio2: Option<p::GPIO6<'static>>,
    lcd_sio3: Option<p::GPIO7<'static>>,
    lcd_cs: Option<p::GPIO12<'static>>,
    dma_ch0: Option<p::DMA_CH0<'static>>,
    lcd_reset: Option<p::GPIO8<'static>>,
    lcd_te: Option<p::GPIO13<'static>>,
    touch_rst: Option<p::GPIO9<'static>>,
    touch_int: Option<p::GPIO38<'static>>,
    btn_boot: Option<p::GPIO0<'static>>,
    rtc_int: Option<p::GPIO39<'static>>,
    imu_int1: Option<p::GPIO21<'static>>,
    flash: Option<p::FLASH<'static>>,
    spi3: Option<p::SPI3<'static>>,
    sd_sck: Option<p::GPIO2<'static>>,
    sd_mosi: Option<p::GPIO1<'static>>,
    sd_miso: Option<p::GPIO3<'static>>,
    sd_cs: Option<p::GPIO17<'static>>,
    lpwr: Option<p::LPWR<'static>>,
}

impl Bringup for S3Bringup {
    type Board = PowerControls<'static>;

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
        let (power, pmu) = PowerControls::init(
            Output::new(self.sys_out.take().unwrap(), Level::Low, OutputConfig::default()),
            Output::new(self.motor.take().unwrap(), Level::Low, OutputConfig::default()),
            i2c,
        )
        .expect("PMU init failed - halting");
        (power, PowerTaskState::new(pmu))
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

    fn make_lcd_te(&mut self) -> Option<Input<'static>> {
        Some(Input::new(
            self.lcd_te.take().unwrap(),
            InputConfig::default().with_pull(Pull::None),
        ))
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

    fn make_store(&mut self) -> Store<'static> {
        Store::init(
            self.flash.take().unwrap(),
            FlashRegion::new(board::FLASH_FS_START, board::FLASH_FS_SIZE),
            self.spi3.take().unwrap(),
            self.sd_sck.take().unwrap(),
            self.sd_mosi.take().unwrap(),
            self.sd_miso.take().unwrap(),
            Output::new(self.sd_cs.take().unwrap(), Level::High, OutputConfig::default()),
        )
    }

    async fn make_sensors(
        &mut self,
        i2c: &mut I2c<'static, Blocking>,
    ) -> (RtcTaskState<'static>, ImuTaskState<'static>) {
        let mut rtc_int = Input::new(
            self.rtc_int.take().unwrap(),
            InputConfig::default().with_pull(Pull::Up),
        );
        let _ = rtc_int.wakeup_enable(true, WakeEvent::LowLevel);
        let rtc_state = RtcTaskState::init(Some(rtc_int), i2c);
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
}

#[esp_rtos::main]
async fn main(spawner: embassy_executor::Spawner) {
    let peripherals = esp_hal::init(
        esp_hal::Config::default().with_cpu_clock(esp_hal::clock::CpuClock::max()),
    );

    // PSRAM allocator (esp-hal 1.1 takes the config via the macro).
    let psram_config = esp_hal::psram::PsramConfig {
        mode: esp_hal::psram::PsramMode::OctalSpi,
        ram_frequency: esp_hal::psram::SpiRamFreq::Freq80m,
        core_clock: Some(esp_hal::psram::SpiTimingConfigCoreClock::SpiTimingConfigCoreClock160m),
        ..Default::default()
    };
    esp_alloc::psram_allocator!(peripherals.PSRAM, esp_hal::psram, psram_config);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_int =
        esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_int.software_interrupt0);
    esp_println::logger::init_logger(log::LevelFilter::Info);
    log::info!("--- ESP32-S3-Touch-AMOLED-2.06 booting ---");

    // All board-specific construction lives in the `Bringup` impl;
    // the shared orchestrator drives the canonical boot sequence.
    let bringup = S3Bringup {
        i2c0: Some(peripherals.I2C0),
        i2c_sda: Some(peripherals.GPIO15),
        i2c_scl: Some(peripherals.GPIO14),
        sys_out: Some(peripherals.GPIO10),
        motor: Some(peripherals.GPIO18),
        spi2: Some(peripherals.SPI2),
        lcd_sclk: Some(peripherals.GPIO11),
        lcd_sio0: Some(peripherals.GPIO4),
        lcd_sio1: Some(peripherals.GPIO5),
        lcd_sio2: Some(peripherals.GPIO6),
        lcd_sio3: Some(peripherals.GPIO7),
        lcd_cs: Some(peripherals.GPIO12),
        dma_ch0: Some(peripherals.DMA_CH0),
        lcd_reset: Some(peripherals.GPIO8),
        lcd_te: Some(peripherals.GPIO13),
        touch_rst: Some(peripherals.GPIO9),
        touch_int: Some(peripherals.GPIO38),
        btn_boot: Some(peripherals.GPIO0),
        rtc_int: Some(peripherals.GPIO39),
        imu_int1: Some(peripherals.GPIO21),
        flash: Some(peripherals.FLASH),
        spi3: Some(peripherals.SPI3),
        sd_sck: Some(peripherals.GPIO2),
        sd_mosi: Some(peripherals.GPIO1),
        sd_miso: Some(peripherals.GPIO3),
        sd_cs: Some(peripherals.GPIO17),
        lpwr: Some(peripherals.LPWR),
    };

    run(bringup, spawner).await
}
