#![no_std]
#![no_main]

//! LilyGo T-Watch Ultra firmware - the full `system_core` port.
//!
//! Mirrors `firmware-s3/src/main.rs` in role: this bin owns the pins,
//! the `Bringup` construction seam and the `Board` impl; the shared
//! `system_core::manager::run` drives the canonical boot sequence and
//! event loop. Board deltas from the S3: touch is a CST92xx whose
//! reset runs through the XL9555 expander (built here, handed to the
//! shared task via `TouchTaskState::with_driver`); the IMU is a
//! BHI260AP the shared QMI8658 task can't drive yet (it probes, logs
//! the miss, and idles - BHI260AP support is a separate effort);
//! haptics are a DRV2605 dispatched through a bin-local task (the
//! `spawn_audio` hook spawns it); and there is no speaker task yet
//! (the MAX98357A needs no codec init but the shared session layer
//! isn't wired). The `smoke` bin (src/bin/smoke.rs) is the
//! standalone hardware diagnostic from bring-up.

extern crate alloc;

mod board;
mod system;

use crate::system::power::TwatchUltraBoard;
use drivers::touch::cst9217::Cst9217;
use drivers::touch::AnyTouch;
use drivers::xl9555::{Config as ExpanderConfig, Xl9555};
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
use embassy_time::{Delay, Duration, Timer};
use esp_backtrace as _;
use esp_hal::gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull, WakeEvent};
use esp_hal::i2c::master::{Config as I2cConfig, I2c};
use esp_hal::peripherals as p;
use esp_hal::time::Rate;
use esp_hal::timer::timg::TimerGroup;
use esp_hal::Blocking;

esp_bootloader_esp_idf::esp_app_desc!();

/// T-Watch Ultra boot-construction seam. Holds the raw peripheral
/// tokens (esp-hal singletons can't be partial-moved through
/// `&mut self`, so each is an `Option` `.take()`-n once by its
/// `Bringup` method).
struct TwatchUltraBringup {
    i2c0: Option<p::I2C0<'static>>,
    i2c_sda: Option<p::GPIO3<'static>>,
    i2c_scl: Option<p::GPIO2<'static>>,
    pmu_irq: Option<p::GPIO7<'static>>,
    // Shared-SPI chip selects held deselected (see TwatchUltraBoard).
    lora_cs: Option<p::GPIO36<'static>>,
    nfc_cs: Option<p::GPIO4<'static>>,
    spi2: Option<p::SPI2<'static>>,
    lcd_sclk: Option<p::GPIO40<'static>>,
    lcd_sio0: Option<p::GPIO38<'static>>,
    lcd_sio1: Option<p::GPIO39<'static>>,
    lcd_sio2: Option<p::GPIO42<'static>>,
    lcd_sio3: Option<p::GPIO45<'static>>,
    lcd_cs: Option<p::GPIO41<'static>>,
    dma_ch0: Option<p::DMA_CH0<'static>>,
    lcd_reset: Option<p::GPIO37<'static>>,
    lcd_te: Option<p::GPIO6<'static>>,
    touch_int: Option<p::GPIO12<'static>>,
    btn_boot: Option<p::GPIO0<'static>>,
    rtc_int: Option<p::GPIO1<'static>>,
    imu_int: Option<p::GPIO8<'static>>,
    flash: Option<p::FLASH<'static>>,
    spi3: Option<p::SPI3<'static>>,
    sd_sck: Option<p::GPIO35<'static>>,
    sd_mosi: Option<p::GPIO34<'static>>,
    sd_miso: Option<p::GPIO33<'static>>,
    sd_cs: Option<p::GPIO21<'static>>,
    lpwr: Option<p::LPWR<'static>>,
    /// Card-detect state read from the XL9555 in `make_power`
    /// (LOW = inserted), consumed by `make_store`.
    sd_present: bool,
}

impl Bringup for TwatchUltraBringup {
    type Board = TwatchUltraBoard;

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
        // External 10K pull-up on the PMU IRQ line; armed for wake so
        // a PWR-button press wakes the watch from light sleep.
        let mut pmu_irq = Input::new(
            self.pmu_irq.take().unwrap(),
            InputConfig::default().with_pull(Pull::None),
        );
        let _ = pmu_irq.wakeup_enable(true, WakeEvent::LowLevel);

        let lora_cs =
            Output::new(self.lora_cs.take().unwrap(), Level::High, OutputConfig::default());
        let nfc_cs =
            Output::new(self.nfc_cs.take().unwrap(), Level::High, OutputConfig::default());

        let (board, pmu) = TwatchUltraBoard::init(i2c, pmu_irq, lora_cs, nfc_cs)
            .expect("PMU init failed - halting");

        // Card detect (XL9555 P12, active low). Read once at boot;
        // make_store uses it to skip the SD probe on an empty slot.
        let expander = Xl9555::new(ExpanderConfig::default());
        self.sd_present = expander
            .read_pin(i2c, board::EXP_SD_DET)
            .map(|level| !level)
            .unwrap_or(false);
        log::info!(
            "SD slot: card {}",
            if self.sd_present { "inserted" } else { "not inserted" },
        );

        (board, PowerTaskState::new(pmu))
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

        // The CST92xx reset line runs through the XL9555, so the
        // shared `TouchTaskState::init` (host-GPIO reset) doesn't
        // apply here: pulse the expander pin (vendor timing), probe
        // the chip, and hand the ready driver to the shared task.
        let expander = Xl9555::new(ExpanderConfig::default());
        let _ = expander.write_pin(i2c, board::EXP_TOUCH_RST, false);
        Timer::after(Duration::from_millis(20)).await;
        let _ = expander.write_pin(i2c, board::EXP_TOUCH_RST, true);
        Timer::after(Duration::from_millis(60)).await;

        let mut cst = Cst9217::new();
        match cst.init(i2c, &mut Delay) {
            Ok(info) => log::info!(
                "Touch: CST{:04X}, fw 0x{:08X}, matrix {}x{}",
                info.chip_id, info.fw_version, info.res_x, info.res_y,
            ),
            Err(()) => log::error!("Touch: CST92xx init failed"),
        }

        let touch = TouchTaskState::with_driver(AnyTouch::Cst92xx(cst), touch_int);
        (touch, BootButtonTaskState::new(boot_btn))
    }

    fn make_store(&mut self) -> Store<'static> {
        // This board has card detect (read in make_power) - an empty
        // slot skips the SD bus entirely. Probing a cardless bus
        // costs ~50 bounded-retry rounds inside embedded-sdmmc's
        // acquire (tens of seconds of boot stall). A card inserted
        // later needs a reboot to be picked up; runtime hotplug via
        // SD_DET is a possible follow-up.
        let region = FlashRegion::new(board::FLASH_FS_START, board::FLASH_FS_SIZE);
        if !self.sd_present {
            return Store::init_flash_only(self.flash.take().unwrap(), region);
        }
        Store::init(
            self.flash.take().unwrap(),
            region,
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

        // No QMI8658 on this board - the shared IMU task probes it,
        // logs the miss, and idles on an INT that never fires. The
        // BHI260AP on GPIO8 needs its own driver (firmware upload at
        // init) - a separate planned effort.
        let imu = ImuTaskState::init(
            Input::new(self.imu_int.take().unwrap(), InputConfig::default().with_pull(Pull::Down)),
            i2c,
        )
        .await;
        (rtc_state, imu)
    }

    fn make_rtc_ctrl(&mut self) -> esp_hal::rtc_cntl::Rtc<'static> {
        esp_hal::rtc_cntl::Rtc::new(self.lpwr.take().unwrap())
    }

    /// No speaker task yet - the MAX98357A amp needs no codec init,
    /// but the shared audio session layer assumes an ES8311/ES7210
    /// pair; the audio effort adds a backend seam there, and until
    /// then AUDIO_COMMANDs are dropped (try_send) with a warn. This
    /// hook is the bin-task spawn point, so the haptics dispatcher
    /// (DRV2605 - needs the shared I2C bus) is spawned here; the
    /// speaker task joins it when audio lands.
    fn spawn_audio(
        &mut self,
        spawner: embassy_executor::Spawner,
        i2c_bus: &'static system_core::bus::SharedI2c,
    ) {
        spawner.spawn(crate::system::haptics::haptics_task(i2c_bus).unwrap());
    }
}

#[esp_rtos::main]
async fn main(spawner: embassy_executor::Spawner) {
    let peripherals = esp_hal::init(
        esp_hal::Config::default().with_cpu_clock(esp_hal::clock::CpuClock::max()),
    );

    // Internal-SRAM heap, same size and model as the other two
    // boards. The 8 MB QSPI PSRAM stays unused (see board.rs).
    esp_alloc::heap_allocator!(size: 128 * 1024);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_int =
        esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_int.software_interrupt0);
    esp_println::logger::init_logger(log::LevelFilter::Info);
    log::info!("--- LilyGo T-Watch Ultra booting ---");

    let bringup = TwatchUltraBringup {
        i2c0: Some(peripherals.I2C0),
        i2c_sda: Some(peripherals.GPIO3),
        i2c_scl: Some(peripherals.GPIO2),
        pmu_irq: Some(peripherals.GPIO7),
        lora_cs: Some(peripherals.GPIO36),
        nfc_cs: Some(peripherals.GPIO4),
        spi2: Some(peripherals.SPI2),
        lcd_sclk: Some(peripherals.GPIO40),
        lcd_sio0: Some(peripherals.GPIO38),
        lcd_sio1: Some(peripherals.GPIO39),
        lcd_sio2: Some(peripherals.GPIO42),
        lcd_sio3: Some(peripherals.GPIO45),
        lcd_cs: Some(peripherals.GPIO41),
        dma_ch0: Some(peripherals.DMA_CH0),
        lcd_reset: Some(peripherals.GPIO37),
        lcd_te: Some(peripherals.GPIO6),
        touch_int: Some(peripherals.GPIO12),
        btn_boot: Some(peripherals.GPIO0),
        rtc_int: Some(peripherals.GPIO1),
        imu_int: Some(peripherals.GPIO8),
        flash: Some(peripherals.FLASH),
        spi3: Some(peripherals.SPI3),
        sd_sck: Some(peripherals.GPIO35),
        sd_mosi: Some(peripherals.GPIO34),
        sd_miso: Some(peripherals.GPIO33),
        sd_cs: Some(peripherals.GPIO21),
        lpwr: Some(peripherals.LPWR),
        sd_present: false,
    };

    run(bringup, spawner).await
}
