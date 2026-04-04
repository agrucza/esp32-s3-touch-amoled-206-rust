#![no_std]
#![no_main]

extern crate alloc;

mod board;
mod display_hal;
mod sdcard_hal;
mod audio_hal;
mod system;
mod tasks;

use drivers::touch::TouchEvent;
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
            .with_psram(esp_hal::psram::PsramConfig {
                ram_frequency: esp_hal::psram::SpiRamFreq::Freq80m,
                core_clock: core::prelude::v1::Some(esp_hal::psram::SpiTimingConfigCoreClock::SpiTimingConfigCoreClock160m),
                ..Default::default()
            })
    );

    let timg0 = TimerGroup::new(p.TIMG0);
    esp_rtos::start(timg0.timer0);
    esp_println::logger::init_logger(log::LevelFilter::Info);
    log::info!("--- ESP32-S3-Touch-AMOLED-2.06 booting ---");

    // Shared I2C bus (PMU, touch, IMU, RTC, codecs)
    let mut i2c = I2c::new(p.I2C0, I2cConfig::default().with_frequency(Rate::from_khz(400)))
        .unwrap()
        .with_sda(p.GPIO15)
        .with_scl(p.GPIO14);

    // Power system - must init first (enables all power rails)
    let mut power = system::power::PowerSystem::init(
        Output::new(p.GPIO10, Level::Low, OutputConfig::default()),
        Output::new(p.GPIO18, Level::Low, OutputConfig::default()),
        &mut i2c,
    ).expect("PMU init failed - halting");
    Timer::after(Duration::from_millis(20)).await;

    // PSRAM allocator - must happen before any alloc
    esp_alloc::psram_allocator!(p.PSRAM, esp_hal::psram);

    // Display
    let mut fb = alloc::vec![0u8; display_hal::FB_BYTES];
    let _display = system::display::init_display(
        p.SPI2, p.GPIO11, p.GPIO4, p.GPIO5, p.GPIO6, p.GPIO7, p.GPIO12,
        p.DMA_CH0,
        Output::new(p.GPIO8, Level::High, OutputConfig::default()),
        &mut fb,
    ).await;

    // Input (buttons + touch)
    let mut input = system::input::InputSystem::init(
        Input::new(p.GPIO0, InputConfig::default().with_pull(Pull::Up)),
        Output::new(p.GPIO9, Level::High, OutputConfig::default()),
        Input::new(p.GPIO38, InputConfig::default().with_pull(Pull::Up)),
        &mut i2c,
    ).await;

    // SD card
    let _vol_mgr = system::storage::init_sd(
        p.SPI3, p.GPIO2, p.GPIO1, p.GPIO3,
        Output::new(p.GPIO17, Level::High, OutputConfig::default()),
    );

    // Sensors (RTC + IMU with gyro calibration)
    let _sensors = system::sensors::SensorSystem::init(&mut i2c).await;

    // Audio (I2S + codecs + DMA)
    let (rx_buffer, rx_descriptors, tx_buffer, tx_descriptors) =
        esp_hal::dma_circular_buffers!(16384, 16384);
    let (_audio, _tx_transfer, mut rx_transfer) = system::audio::init_audio(
        p.I2S0, p.DMA_CH1,
        p.GPIO16, p.GPIO41, p.GPIO45, p.GPIO40, p.GPIO42,
        Output::new(p.GPIO46, Level::Low, OutputConfig::default()),
        tx_buffer, rx_buffer, tx_descriptors, rx_descriptors,
        &mut i2c,
    ).await;

    spawner.must_spawn(tasks::heartbeat::heartbeat());

    // Main loop
    loop {
        // Drain mic RX buffer to prevent overflow
        {
            let mut pcm = alloc::vec![0u8; 16384];
            let _ = rx_transfer.pop(&mut pcm).await;
        }

        // Input polling
        if input.poll_boot_button() {
            log::info!("BTN: BOOT pressed");
            power.buzz();
            Timer::after(Duration::from_millis(200)).await;
            power.buzz_stop();
        }

        power.poll_pwr_button(&mut i2c);

        match input.poll_touch(&mut i2c) {
            TouchEvent::Pressed { x, y } => log::info!("Touch: ({}, {})", x, y),
            TouchEvent::Released => log::info!("Touch: released"),
            TouchEvent::None => {}
        }

        Timer::after(Duration::from_millis(10)).await;
    }
}
