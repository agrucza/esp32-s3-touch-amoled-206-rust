#![no_std]
#![no_main]

extern crate alloc;

mod board;
// The audio stack (ES8311 DAC, ES7210 ADC, I2S DMA, speaker amp)
// lives in `system_core::audio` - same hardware on every board.
// It's implemented but not wired into the manager loop yet (idle
// current); bring-up is reintroduced bin-side when there's a caller.
mod system;
mod tasks;

// `config`, `events`, and `ui` live in the `app-core` crate so they
// can be host-tested. Re-export at the crate root so existing
// `crate::config::...` paths keep working.
pub use app_core::{config, events, ui};

use crate::system::power::PowerControls;
use system_core::manager::SystemManager;
use system_core::storage::Store;
use system_core::flash_fs::FlashRegion;
use system_core::tasks::{
    boot_button::{boot_button_task, BootButtonTaskState},
    imu::{imu_task, ImuTaskState},
    power::{power_task, PowerTaskState},
    rtc::{rtc_task, RtcTaskState},
    touch::{touch_task, TouchTaskState},
};
use esp_backtrace as _;
use esp_hal::gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull};
use esp_hal::i2c::master::{Config as I2cConfig, I2c};
use esp_hal::time::Rate;
use esp_hal::timer::timg::TimerGroup;
use embassy_time::{Duration, Timer};

esp_bootloader_esp_idf::esp_app_desc!();

#[esp_rtos::main]
async fn main(spawner: embassy_executor::Spawner) {
    let p = esp_hal::init(
        esp_hal::Config::default()
            .with_cpu_clock(esp_hal::clock::CpuClock::max())
    );

    // PSRAM allocator (esp-hal 1.1 takes the config via the macro).
    let psram_config = esp_hal::psram::PsramConfig {
        mode: esp_hal::psram::PsramMode::OctalSpi,
        ram_frequency: esp_hal::psram::SpiRamFreq::Freq80m,
        core_clock: Some(esp_hal::psram::SpiTimingConfigCoreClock::SpiTimingConfigCoreClock160m),
        ..Default::default()
    };
    esp_alloc::psram_allocator!(p.PSRAM, esp_hal::psram, psram_config);

    let timg0 = TimerGroup::new(p.TIMG0);
    let sw_int = esp_hal::interrupt::software::SoftwareInterruptControl::new(p.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_int.software_interrupt0);
    esp_println::logger::init_logger(log::LevelFilter::Info);
    log::info!("--- ESP32-S3-Touch-AMOLED-2.06 booting ---");

    // ---- Hardware construction (board-specific; concrete pins) -----------
    //
    // The shared `system-core` brain is built once below from these
    // constructed pieces via `SystemManager::new`. Order matters:
    // I2C first (shared bus), power first peripheral (enables rails).

    // 1. I2C bus.
    let mut i2c = I2c::new(p.I2C0, I2cConfig::default().with_frequency(Rate::from_khz(400)))
        .unwrap()
        .with_sda(p.GPIO15)
        .with_scl(p.GPIO14);

    // 2. Power: GPIO latch + motor + AXP2101 init. `PowerControls`
    //    is this board's `system_core::board::Board` impl.
    let (board, pmu) = PowerControls::init(
        Output::new(p.GPIO10, Level::Low, OutputConfig::default()),
        Output::new(p.GPIO18, Level::Low, OutputConfig::default()),
        &mut i2c,
    ).expect("PMU init failed - halting");
    let power_state = PowerTaskState::new(pmu);
    Timer::after(Duration::from_millis(20)).await;

    // 3. Framebuffer (internal-SRAM BSS, shared display HAL).
    let fb: &'static mut [u8] = firmware_hal::display::take_framebuffer();

    // 4. Display.
    let display = system_core::display::init_display(
        p.SPI2, p.GPIO11, p.GPIO4, p.GPIO5,
        p.GPIO6, p.GPIO7, p.GPIO12, p.DMA_CH0,
        Output::new(p.GPIO8, Level::High, OutputConfig::default()),
        fb,
    ).await;

    // 5. TE input (this board has the GPIO; `None` boards skip the wait).
    let lcd_te = Input::new(p.GPIO13, InputConfig::default().with_pull(Pull::None));

    // 6. Touch + BOOT button. Arm RTC-wake on the INT pins before
    //    handing them to the tasks.
    let mut touch_int = Input::new(p.GPIO38, InputConfig::default().with_pull(Pull::Up));
    let mut boot_btn  = Input::new(p.GPIO0,  InputConfig::default().with_pull(Pull::Up));
    let mut rtc_int   = Input::new(p.GPIO39, InputConfig::default().with_pull(Pull::Up));
    let _ = touch_int.wakeup_enable(true, esp_hal::gpio::WakeEvent::LowLevel);
    let _ = boot_btn.wakeup_enable(true, esp_hal::gpio::WakeEvent::LowLevel);
    let _ = rtc_int.wakeup_enable(true, esp_hal::gpio::WakeEvent::LowLevel);

    let touch_state = TouchTaskState::init(
        Output::new(p.GPIO9, Level::High, OutputConfig::default()),
        touch_int,
        &mut i2c,
    ).await;
    let boot_button_state = BootButtonTaskState::new(boot_btn);

    // 7. Persistent storage: flash + SD. The region geometry is this
    //    bin's (kept in sync with partitions-s3.csv via board.rs).
    let store = Store::init(
        p.FLASH,
        FlashRegion::new(board::FLASH_FS_START, board::FLASH_FS_SIZE),
        p.SPI3, p.GPIO2, p.GPIO1, p.GPIO3,
        Output::new(p.GPIO17, Level::High, OutputConfig::default()),
    );

    // 8. Sensors (RTC + IMU; IMU does ~512 ms gyro-bias calibration).
    let rtc_state = RtcTaskState::init(rtc_int, &mut i2c);
    let imu_state = ImuTaskState::init(
        Input::new(p.GPIO21, InputConfig::default().with_pull(Pull::Down)),
        &mut i2c,
    ).await;

    // 9. Initial snapshots (need raw &mut i2c before it's moved into
    //    the global mutex).
    let initial_time = rtc_state.snapshot(&mut i2c);
    let initial_power = power_state.snapshot(&mut i2c);
    power_state.dump_status(&mut i2c);

    // 10. Move the I2C bus into the global mutex shared with tasks.
    let i2c_bus: &'static system_core::bus::SharedI2c =
        system_core::bus::I2C_BUS.init(embassy_sync::mutex::Mutex::new(i2c));

    // 11. RTC controller for hardware light sleep.
    let rtc = esp_hal::rtc_cntl::Rtc::new(p.LPWR);

    // ---- Assemble the shared brain --------------------------------------
    let (mut manager, bundle) = SystemManager::new(
        i2c_bus,
        board,
        display,
        Some(lcd_te),
        rtc,
        store,
        initial_time,
        initial_power,
        touch_state,
        boot_button_state,
        rtc_state,
        imu_state,
        power_state,
    );

    // Each task is spawned exactly once at boot; `.unwrap()` is the
    // right "must succeed" shape (embassy-executor 0.10 returns a
    // `Result<SpawnToken, SpawnError>` from the task macro).
    spawner.spawn(tasks::heartbeat::heartbeat().unwrap());
    spawner.spawn(touch_task(i2c_bus, bundle.touch).unwrap());
    spawner.spawn(boot_button_task(bundle.boot_button).unwrap());
    spawner.spawn(rtc_task(i2c_bus, bundle.rtc).unwrap());
    spawner.spawn(imu_task(i2c_bus, bundle.imu).unwrap());
    spawner.spawn(power_task(i2c_bus, bundle.power).unwrap());

    loop {
        manager.tick().await;
    }
}
