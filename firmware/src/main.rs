#![no_std]
#![no_main]

mod board;
mod display_hal;
mod tasks;

use display_hal::{CO5300, color};
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
    let p = esp_hal::init(esp_hal::Config::default());

    // esp_rtos::start only needs the timer on Xtensa — the software interrupt
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
            log::info!("PMU: AXP2101 rev {} (0x{:02X}) — all rails enabled", version, raw_id);
        }
        Err(_) => {
            log::error!("PMU: initialization failed — halting");
            loop {}
        }
    }
    Timer::after(Duration::from_millis(20)).await; // let rails stabilise

    // --- Display setup ---
    let spi = display_hal::build_spi(
        p.SPI2,
        p.GPIO11, // LCD_SCLK
        p.GPIO4,  // LCD_SDIO0
        p.GPIO5,  // LCD_SDIO1
        p.GPIO6,  // LCD_SDIO2
        p.GPIO7,  // LCD_SDIO3
        p.GPIO12, // LCD_CS
    );
    let reset = Output::new(p.GPIO8, Level::High, OutputConfig::default());
    let mut display = CO5300::new(spi, reset);

    // Hardware reset: short low pulse then settle
    display.reset_high();
    Timer::after(Duration::from_millis(10)).await;
    display.reset_low();
    Timer::after(Duration::from_millis(10)).await;
    display.reset_high();
    Timer::after(Duration::from_millis(120)).await; // controller boot time

    // Password unlock/lock + SPI mode + pixel format (no delays needed)
    log::info!("Main: Starting display init");
    display.init();
    log::info!("Main: Display init complete");

    // Sleep out — must come AFTER config, BEFORE display on
    log::info!("Main: Sending SLPOUT command");
    display.wake();
    Timer::after(Duration::from_millis(120)).await;

    // Display on
    log::info!("Main: Sending DISPON command");
    display.display_on();
    Timer::after(Duration::from_millis(70)).await;

    log::info!("Display initialised — filling screen red");
    display.fill_solid(0, 0, display_hal::WIDTH, display_hal::HEIGHT, color::RED);
    log::info!("Fill done");

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
        Err(_) => log::error!("Touch: device not found at I²C address 0x{:02X}", drivers::touch::ADDR),
    }

    // --- RTC setup ---
    log::info!("RTC: initializing PCF85063...");
    let rtc = Rtc::new(RtcConfig::default());
    match rtc.init(&mut i2c) {
        Err(_) => log::error!("RTC: device not found on I²C bus"),
        Ok(os_flag) => {
            if os_flag {
                log::warn!("RTC: oscillator-stop flag set — time is invalid");
            } else {
                log::info!("RTC: oscillator running, time is valid");
            }

            // Need to set time if oscillator stopped OR if the values are garbage.
            // Some PCF85063 variants don't set the OS flag on first power-up,
            // so we validate the read-back values as a second check.
            let needs_set = os_flag || match rtc.get(&mut i2c) {
                Ok(ref dt) => !dt.is_valid(),
                Err(_)     => true,
            };

            if needs_set {
                log::warn!("RTC: time invalid — setting default");
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

    spawner.must_spawn(tasks::heartbeat::heartbeat());

    let colors = [color::RED, color::GREEN, color::BLUE];
    let mut color_index = 0;

    // Read when INT is low (data ready) OR when a finger was last seen down.
    // The second condition catches lift-off: FT3168 briefly pulses INT low on
    // release, which a 20 ms poll can miss. Continuing to read until count=0
    // guarantees the Released event is never skipped.
    loop {
        // We will shift the display color around a bit
        log::info!("Updating display to color index: {}", color_index);
        display.fill_solid(0, 0, display_hal::WIDTH, display_hal::HEIGHT, colors[color_index]);
        color_index = (color_index+1)%colors.len();
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
    }
}
