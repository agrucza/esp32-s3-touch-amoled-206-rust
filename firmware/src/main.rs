#![no_std]
#![no_main]

extern crate alloc;

mod board;
mod display_hal;
mod events;
mod sdcard_hal;
mod audio_hal;
mod system;
mod tasks;
mod ui;

use system::manager::{SystemManager, Peripherals};
use esp_backtrace as _;
use esp_hal::timer::timg::TimerGroup;

esp_bootloader_esp_idf::esp_app_desc!();

#[esp_rtos::main]
async fn main(spawner: embassy_executor::Spawner) {
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

    // DMA buffers must be created here (macro produces statics)
    let (rx_buffer, rx_descriptors, tx_buffer, tx_descriptors) =
        esp_hal::dma_circular_buffers!(16384, 16384);

    let mut manager = SystemManager::init(Peripherals {
        i2c0: p.I2C0,
        i2c_sda: p.GPIO15,
        i2c_scl: p.GPIO14,
        psram: p.PSRAM,
        sys_out_pin: p.GPIO10,
        motor_pin: p.GPIO18,
        spi2: p.SPI2,
        lcd_sclk: p.GPIO11,
        lcd_sio0: p.GPIO4,
        lcd_sio1: p.GPIO5,
        lcd_sio2: p.GPIO6,
        lcd_sio3: p.GPIO7,
        lcd_cs: p.GPIO12,
        dma_ch0: p.DMA_CH0,
        lcd_reset: p.GPIO8,
        btn_boot: p.GPIO0,
        touch_rst: p.GPIO9,
        touch_int: p.GPIO38,
        spi3: p.SPI3,
        sd_sck: p.GPIO2,
        sd_mosi: p.GPIO1,
        sd_miso: p.GPIO3,
        sd_cs: p.GPIO17,
        i2s0: p.I2S0,
        dma_ch1: p.DMA_CH1,
        audio_mclk: p.GPIO16,
        audio_bclk: p.GPIO41,
        audio_ws: p.GPIO45,
        audio_dout: p.GPIO40,
        audio_din: p.GPIO42,
        audio_pa: p.GPIO46,
        tx_buffer,
        rx_buffer,
        tx_descriptors,
        rx_descriptors,
    }).await;

    spawner.must_spawn(tasks::heartbeat::heartbeat());

    loop {
        manager.tick().await;
    }
}
