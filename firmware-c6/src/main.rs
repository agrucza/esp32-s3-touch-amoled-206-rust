#![no_std]
#![no_main]

extern crate alloc;

mod board;

use drivers::display::color;
use drivers::pmu::{Config as PmuConfig, Pmu};
use embassy_time::{Duration, Timer};
use embedded_hal::i2c::I2c as I2cTrait;
use esp_backtrace as _;
use esp_hal::{
    gpio::{Level, Output, OutputConfig},
    i2c::master::{Config as I2cConfig, I2c},
    time::Rate,
    timer::timg::TimerGroup,
};
use firmware_hal::display as display_hal;

esp_bootloader_esp_idf::esp_app_desc!();

// Tie the literal GPIO bindings in `main` back to the board pin map.
// If the schematic-derived constants drift away from the literal
// peripheral fields, the build fails instead of silently miswiring.
const _: () = {
    assert!(board::I2C_SDA  ==  8, "board::I2C_SDA must match the GPIO8 binding below");
    assert!(board::I2C_SCL  ==  7, "board::I2C_SCL must match the GPIO7 binding below");
    assert!(board::LCD_SCLK ==  0, "board::LCD_SCLK must match the GPIO0 binding below");
    assert!(board::LCD_SDIO0 == 1, "board::LCD_SDIO0 must match the GPIO1 binding below");
    assert!(board::LCD_SDIO1 == 2, "board::LCD_SDIO1 must match the GPIO2 binding below");
    assert!(board::LCD_SDIO2 == 3, "board::LCD_SDIO2 must match the GPIO3 binding below");
    assert!(board::LCD_SDIO3 == 4, "board::LCD_SDIO3 must match the GPIO4 binding below");
    assert!(board::LCD_CS    == 5, "board::LCD_CS must match the GPIO5 binding below");
    assert!(board::LCD_RESET == 11, "board::LCD_RESET must match the GPIO11 binding below");
};

/// Number of panel rows our partial framebuffer covers.
/// 40 rows * 410 cols * 2 bytes/pixel = 32,800 bytes.
/// Smoke-test size; will be retuned (likely to LVGL's ~50 rows) once we
/// move to the long-term partial-FB rendering strategy.
const FB_ROWS: u16 = 40;

/// Expected I2C devices on the C6 shared bus.
/// Used at scan time to flag any device that doesn't ACK.
const EXPECTED: [(u8, &str); 6] = [
    (0x18, "ES8311 codec"),
    (0x34, "AXP2101 PMU"),
    (board::TOUCH_I2C_ADDR, "FT3168 touch"),
    (0x40, "ES7210 mic ADC"),
    (0x51, "PCF85063 RTC"),
    (0x6B, "QMI8658 IMU"),
];

fn device_name(addr: u8) -> &'static str {
    EXPECTED
        .iter()
        .find_map(|(a, name)| if *a == addr { Some(*name) } else { None })
        .unwrap_or("unknown")
}

/// Probe every 7-bit address with a 1-byte read and report ACKs.
/// Side-effect-free: no register pointer gets written on the slave side.
fn scan_and_report<I2C, E>(i2c: &mut I2C, label: &str)
where
    I2C: I2cTrait<Error = E>,
{
    log::info!("{}", label);
    let mut mask: u128 = 0;
    let mut count: u32 = 0;
    for addr in 0x08u8..0x78 {
        let mut byte = [0u8; 1];
        if i2c.read(addr, &mut byte).is_ok() {
            mask |= 1u128 << addr;
            count += 1;
        }
    }
    log::info!("  {} device(s) ACKed", count);
    for addr in 0x08u8..0x78 {
        if (mask >> addr) & 1 == 1 {
            log::info!("    0x{:02X}  {}", addr, device_name(addr));
        }
    }
    for &(addr, name) in EXPECTED.iter() {
        if (mask >> addr) & 1 == 0 {
            log::warn!("    0x{:02X}  MISSING - expected {}", addr, name);
        }
    }
}

#[esp_rtos::main]
async fn main(_spawner: embassy_executor::Spawner) {
    let p = esp_hal::init(esp_hal::Config::default());

    // Internal-SRAM heap. The C6 has no PSRAM, so the framebuffer (32 KB
    // at FB_ROWS = 40) lives here alongside any other dynamic allocation.
    // 64 KB total gives us comfortable headroom past the FB.
    esp_alloc::heap_allocator!(size: 64 * 1024);

    let timg0 = TimerGroup::new(p.TIMG0);
    let sw_int = esp_hal::interrupt::software::SoftwareInterruptControl::new(p.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_int.software_interrupt0);

    esp_println::logger::init_logger(log::LevelFilter::Info);
    log::info!("--- ESP32-C6-Touch-AMOLED-2.06 ---");

    // I2C bus: GPIO7 (SCL) / GPIO8 (SDA), 400 kHz.
    // All six on-board peripherals share this bus.
    let mut i2c = I2c::new(
        p.I2C0,
        I2cConfig::default().with_frequency(Rate::from_khz(400)),
    )
    .unwrap()
    .with_sda(p.GPIO8)
    .with_scl(p.GPIO7);

    // -- AXP2101 sanity ping ----------------------------------------------
    // We don't configure any rails: the AXP retains its rail state across
    // MCU resets (runs from VBUS/VBAT independently), and Waveshare's
    // reference firmware never enables rails in software either. We just
    // confirm the chip is reachable and log the chip ID.
    let pmu = Pmu::new(PmuConfig::default());
    match pmu.check_device(&mut i2c) {
        Ok(chip_id) => log::info!(
            "AXP2101 chip ID: 0x{:02X} (rev {:02b})",
            chip_id,
            (chip_id >> 4) & 0x03,
        ),
        Err(_) => log::error!("AXP2101 check_device failed - cannot continue safely"),
    }

    // -- Wait for FT3168 touch to wake from its persisted MONITOR mode ----
    // The touch IC is left in TOUCH_POWER_MONITOR by the previous firmware
    // and does not ACK its I2C address for ~2 s after boot. Poll until it
    // shows up. See project_c6_ft3168_wake_delay.md in memory.
    const TOUCH_WAKE_TIMEOUT_MS: u32 = 5000;
    const TOUCH_POLL_INTERVAL_MS: u32 = 500;
    log::info!(
        "Waiting for FT3168 (0x{:02X}) to wake (poll every {} ms, timeout {} ms)...",
        board::TOUCH_I2C_ADDR, TOUCH_POLL_INTERVAL_MS, TOUCH_WAKE_TIMEOUT_MS,
    );
    let mut elapsed_ms: u32 = 0;
    let mut touch_awake = false;
    while elapsed_ms < TOUCH_WAKE_TIMEOUT_MS {
        Timer::after(Duration::from_millis(TOUCH_POLL_INTERVAL_MS as u64)).await;
        elapsed_ms += TOUCH_POLL_INTERVAL_MS;
        let mut byte = [0u8; 1];
        if i2c.read(board::TOUCH_I2C_ADDR, &mut byte).is_ok() {
            log::info!("  FT3168 awake after {} ms", elapsed_ms);
            touch_awake = true;
            break;
        }
    }
    if !touch_awake {
        log::warn!("  FT3168 did not ACK within {} ms", TOUCH_WAKE_TIMEOUT_MS);
    }

    // -- Final bus scan ---------------------------------------------------
    scan_and_report(&mut i2c, "Bus scan:");

    // -- Display bring-up (partial framebuffer, smoke test) ---------------
    // Allocate the partial FB in internal SRAM via the esp-alloc heap set
    // up above. `Vec::leak()` gives us a `&'static mut [u8]` we can hand
    // to the CO5300 driver, mirroring how the S3 firmware leaks a Vec
    // out of its PSRAM allocator.
    let fb_bytes = display_hal::fb_bytes_for_rows(FB_ROWS);
    let fb: &'static mut [u8] = alloc::vec![0u8; fb_bytes].leak();
    log::info!("Display: allocated {} byte FB ({} rows)", fb_bytes, FB_ROWS);

    let mut display = display_hal::init_display(
        p.SPI2,
        p.GPIO0,   // LCD_SCLK
        p.GPIO1,   // LCD_SDIO0
        p.GPIO2,   // LCD_SDIO1
        p.GPIO3,   // LCD_SDIO2
        p.GPIO4,   // LCD_SDIO3
        p.GPIO5,   // LCD_CS
        p.DMA_CH0,
        Output::new(p.GPIO11, Level::High, OutputConfig::default()),
        fb,
    )
    .await;

    // Fill the entire FB region (top FB_ROWS rows of the panel) with red
    // and push it to the panel. If everything is wired correctly we
    // should see a red stripe across the top of the screen.
    display.fill_solid(0, 0, display_hal::WIDTH, FB_ROWS, color::RED);
    display.flush_rows(0, FB_ROWS).await;
    log::info!("Display: pushed {} rows of solid red - smoke test complete", FB_ROWS);

    // Heartbeat so we can see the firmware is alive after init.
    let mut counter: u32 = 0;
    loop {
        log::info!("heartbeat #{}", counter);
        counter = counter.wrapping_add(1);
        Timer::after(Duration::from_secs(5)).await;
    }
}
