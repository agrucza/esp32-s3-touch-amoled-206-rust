#![no_std]
#![no_main]

extern crate alloc;

mod board;
mod display_hal;
mod tasks;

use display_hal::{CO5300, color};
use drivers::imu::{Qmi8658, Config as ImuConfig};
use drivers::touch::FT3168;
use drivers::pmu::{Pmu, Config as PmuConfig};
use drivers::rtc::{Rtc, Config as RtcConfig, DateTime as RtcDateTime};
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_backtrace as _;
use esp_hal::{
    gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull},
    i2c::master::{Config as I2cConfig, I2c},
    time::Rate,
    timer::timg::TimerGroup,
};

esp_bootloader_esp_idf::esp_app_desc!();

#[esp_rtos::main]
async fn main(spawner: Spawner) {
    let p = esp_hal::init(
        esp_hal::Config::default()
            .with_cpu_clock(esp_hal::clock::CpuClock::max())
            //.with_psram(esp_hal::psram::PsramConfig::default())
            .with_psram(esp_hal::psram::PsramConfig {
                ram_frequency: esp_hal::psram::SpiRamFreq::Freq80m,
                core_clock: core::prelude::v1::Some(esp_hal::psram::SpiTimingConfigCoreClock::SpiTimingConfigCoreClock160m),
                ..Default::default()
            })
    );

    // esp_rtos::start only needs the timer on Xtensa - the software interrupt
    // argument is RISC-V only (#[cfg(riscv)] in esp-rtos source).
    let timg0 = TimerGroup::new(p.TIMG0);
    esp_rtos::start(timg0.timer0);

    esp_println::logger::init_logger(log::LevelFilter::Info);
    log::info!("--- ESP32-S3-Touch-AMOLED-2.06 booting ---");

    // --- PMU: power on all rails before display or peripherals ---
    let mut i2c = I2c::new(
        p.I2C0,
        I2cConfig::default().with_frequency(Rate::from_khz(400)),
    )
    .unwrap()
    .with_sda(p.GPIO15)
    .with_scl(p.GPIO14);

    let pmu = Pmu::new(PmuConfig::default());
    log::info!("PMU: initializing AXP2101...");
    match pmu.init(&mut i2c) {
        Ok(raw_id) => {
            let version = (raw_id >> 4) & 0x03; // bits 5:4 = chip_version
            log::info!("PMU: AXP2101 rev {} (0x{:02X}) - all rails enabled", version, raw_id);
        }
        Err(_) => {
            log::error!("PMU: initialization failed - halting");
            loop {}
        }
    }
    Timer::after(Duration::from_millis(20)).await; // let rails stabilise

    // --- Display setup ---
    let bus = display_hal::build_spi(
        p.SPI2,
        p.GPIO11,  // LCD_SCLK
        p.GPIO4,   // LCD_SDIO0
        p.GPIO5,   // LCD_SDIO1
        p.GPIO6,   // LCD_SDIO2
        p.GPIO7,   // LCD_SDIO3
        p.GPIO12,  // LCD_CS
        p.DMA_CH0, // GDMA channel for SPI transfers
    );
    let reset = Output::new(p.GPIO8, Level::High, OutputConfig::default());


    // Initialize PSRAM and point the global allocator at it.
    // Must happen before any alloc::vec! or Box::new calls.
    esp_alloc::psram_allocator!(p.PSRAM, esp_hal::psram);

    // Allocate the ~400 KB framebuffer from PSRAM.
    let mut fb = alloc::vec![0u8; display_hal::FB_BYTES];

    let stats = esp_alloc::HEAP.stats();
    log::info!("{}", stats);

    let mut display = CO5300::new(bus, reset, &mut fb);

    // Hardware reset: short low pulse then settle
    display.reset_high();
    Timer::after(Duration::from_millis(10)).await;
    display.reset_low();
    Timer::after(Duration::from_millis(10)).await;
    display.reset_high();
    Timer::after(Duration::from_millis(120)).await; // controller boot time

    log::info!("Main: Starting display init");
    display.init().await;
    log::info!("Main: Display init complete");

    // Sleep out - must come AFTER config, BEFORE display on
    log::info!("Main: Sending SLPOUT command");
    display.wake().await;
    Timer::after(Duration::from_millis(120)).await;

    // Display on
    log::info!("Main: Sending DISPON command");
    display.display_on().await;
    Timer::after(Duration::from_millis(70)).await;

    log::info!("Display initialised - filling screen red");
    display.fill_solid(0, 0, display_hal::WIDTH, display_hal::HEIGHT, color::RED);
    display.flush().await;
    log::info!("Flush done");

    // --- Touch setup ---
    let touch_rst = Output::new(p.GPIO9, Level::High, OutputConfig::default());
    let touch_int = Input::new(p.GPIO38, InputConfig::default().with_pull(Pull::Up));
    let mut touch = FT3168::new(touch_rst);

    touch.reset_low();
    Timer::after(Duration::from_millis(10)).await;
    touch.reset_high();
    Timer::after(Duration::from_millis(50)).await;

    log::info!("Touch: initializing FT3168...");
    match touch.read_ids(&mut i2c) {
        Ok((chip_id, fw_ver)) => log::info!("Touch: chip ID=0x{:02X}, FW version=0x{:02X}", chip_id, fw_ver),
        Err(_) => log::error!("Touch: device not found at I2C address 0x{:02X}", drivers::touch::ADDR),
    }

    // --- RTC setup ---
    log::info!("RTC: initializing PCF85063...");
    let rtc = Rtc::new(RtcConfig::default());
    match rtc.init(&mut i2c) {
        Err(_) => log::error!("RTC: device not found on I2C bus"),
        Ok(os_flag) => {
            if os_flag {
                log::warn!("RTC: oscillator-stop flag set - time is invalid");
            } else {
                log::info!("RTC: oscillator running, time is valid");
            }

            let needs_set = os_flag || match rtc.get(&mut i2c) {
                Ok(ref dt) => !dt.is_valid(),
                Err(_)     => true,
            };

            if needs_set {
                log::warn!("RTC: time invalid - setting default");
                let default_time = RtcDateTime::new(2026, 3, 30, 0, 12, 0, 0);
                if rtc.set(&mut i2c, &default_time).is_err() {
                    log::error!("RTC: failed to set time");
                }
            }

            match rtc.get(&mut i2c) {
                Ok(dt) => log::info!("RTC: {:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                    dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second),
                Err(_) => log::error!("RTC: failed to read time"),
            }
        }
    }

    // --- IMU setup ---
    log::info!("IMU: initializing QMI8658C...");
    let imu_config = ImuConfig::default();
    let mut imu = Qmi8658::new(ImuConfig::default());
    match imu.init(&mut i2c, &imu_config) {
        Err(_) => log::error!("IMU: device not found at I2C address 0x{:02X}", drivers::imu::ADDR),
        Ok(()) => {
            match imu.read_ids(&mut i2c) {
                Ok((chip_id, rev)) => log::info!("IMU: QMI8658C chip_id=0x{:02X} rev=0x{:02X}", chip_id, rev),
                Err(_)             => log::warn!("IMU: init OK but failed to read IDs"),
            }

            // Wait for the gyroscope to fully wake up before sampling bias.
            // Datasheet: gyro turn-on time = 60 ms + 3/ODR (at 125 Hz = ~84 ms).
            Timer::after(Duration::from_millis(100)).await;

            // 64 samples at 125 Hz ODR = ~512 ms - keep the device still.
            log::info!("IMU: collecting gyro bias (keep device still ~512ms)...");
            match imu.collect_gyro_bias(&mut i2c, 64) {
                Err(_) => log::error!("IMU: failed to collect gyro bias"),
                Ok((bx, by, bz)) => {
                    log::info!("IMU: gyro bias raw [{} {} {}]", bx, by, bz);
                    imu.set_gyro_bias(bx, by, bz);
                    log::info!("IMU: gyro bias applied (software)");
                }
            }
        }
    }

    spawner.must_spawn(tasks::heartbeat::heartbeat());

    let colors = [color::RED, color::GREEN, color::BLUE];
    let mut color_index = 0;

    loop {
        log::info!("Updating display to color index: {}", color_index);
        display.fill_solid(0, 0, display_hal::WIDTH, display_hal::HEIGHT, colors[color_index]);
        display.flush().await;
        color_index = (color_index + 1) % colors.len();

        for _ in 0..50 {
            Timer::after(Duration::from_millis(20)).await;
            if touch_int.is_low() || touch.is_pressed() {
                match touch.read(&mut i2c) {
                    drivers::touch::TouchEvent::Pressed { x, y } => log::info!("Touch: ({}, {})", x, y),
                    drivers::touch::TouchEvent::Released         => log::info!("Touch: released"),
                    drivers::touch::TouchEvent::None             => {}
                }
            }
        }

        match imu.read(&mut i2c) {
            Ok(data) => {
                let scale_a = imu.accel_scale().lsb_per_g() as i32;
                let scale_g = imu.gyro_scale().lsb_per_dps() as i32;
                // Scale to milli-g and milli-dps to avoid floating point.
                let ax = data.accel_x as i32 * 1000 / scale_a;
                let ay = data.accel_y as i32 * 1000 / scale_a;
                let az = data.accel_z as i32 * 1000 / scale_a;
                let gx = data.gyro_x  as i32 * 1000 / scale_g;
                let gy = data.gyro_y  as i32 * 1000 / scale_g;
                let gz = data.gyro_z  as i32 * 1000 / scale_g;
                log::info!(
                    "IMU: accel [{:6} {:6} {:6}] mg  gyro [{:7} {:7} {:7}] mdps  temp {}°C",
                    ax, ay, az, gx, gy, gz, data.temp_celsius()
                );
            }
            Err(_) => log::warn!("IMU: read failed"),
        }
    }
}
