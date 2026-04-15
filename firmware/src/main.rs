#![no_std]
#![no_main]

extern crate alloc;

mod board;
mod config;
mod display_hal;
mod events;
mod sdcard_hal;
// Audio stack is fully implemented and stays in the tree but is not
// wired into SystemManager by default - the I2S DMA, DAC, ADC, and
// speaker amp draw significant current when running idle, and we
// don't have an audio use case yet. When audio work begins, re-add
// the audio peripheral tokens in Peripherals, call init_audio in
// SystemManager::init, and drain the mic DMA in tick.
//
// IMPORTANT: the ES8311 / ES7210 analog supply (A3V3) is fed by
// AXP2101 ALDO1 and is held OFF at boot by `Pmu::init`. Before any
// codec / ADC I²C access, the audio bring-up path MUST enable
// ALDO1 via `Pmu::set_audio_rail(true)` and wait ~10 ms for the
// rail to stabilise. `SystemManager::start_audio` already does
// this - don't bypass it. See `drivers/src/pmu/mod.rs` module
// comment and `Pmu::set_audio_rail` docs for the full contract.
#[allow(dead_code)]
mod audio_hal;
mod system;
mod tasks;
mod ui;

use system::manager::{SystemManager, Peripherals};
use system::tasks::{
    boot_button::boot_button_task,
    imu::imu_task,
    power::power_task,
    rtc::rtc_task,
    touch::touch_task,
};
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

    let (mut manager, bundle) = SystemManager::init(Peripherals {
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
        lcd_te: p.GPIO13,
        btn_boot: p.GPIO0,
        touch_rst: p.GPIO9,
        touch_int: p.GPIO38,
        rtc_int: p.GPIO39,
        imu_int1: p.GPIO21,
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

    // Spawn one embassy task per peripheral. Each receives a
    // `&'static` reference to the shared I2C bus (four of the five
    // need it; boot_button is pure GPIO) plus its own task state
    // struct out of the bundle. After this point the main loop only
    // drains events - it never touches a peripheral driver directly.
    let i2c_bus = manager.i2c_bus();
    spawner.must_spawn(touch_task(i2c_bus, bundle.touch));
    spawner.must_spawn(boot_button_task(bundle.boot_button));
    spawner.must_spawn(rtc_task(i2c_bus, bundle.rtc));
    spawner.must_spawn(imu_task(i2c_bus, bundle.imu));
    spawner.must_spawn(power_task(i2c_bus, bundle.power));

    loop {
        manager.tick().await;
    }
}
