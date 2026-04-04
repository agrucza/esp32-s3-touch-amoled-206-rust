use crate::display_hal::{self, CO5300, EspQspi};
use embassy_time::{Duration, Timer};
use esp_hal::{
    dma::DmaChannelFor,
    gpio::{Output, interconnect::PeripheralOutput},
    spi::master::AnySpi,
};

/// Perform the full display hardware init sequence.
///
/// Builds the QSPI bus, performs the reset pulse, sends init commands,
/// sleep out, and display on. Returns the ready-to-use display handle.
pub async fn init_display<'d, 'fb>(
    spi: impl esp_hal::spi::master::Instance + 'd,
    sclk: impl PeripheralOutput<'d>,
    sio0: impl PeripheralOutput<'d>,
    sio1: impl PeripheralOutput<'d>,
    sio2: impl PeripheralOutput<'d>,
    sio3: impl PeripheralOutput<'d>,
    cs: impl PeripheralOutput<'d>,
    dma: impl DmaChannelFor<AnySpi<'d>>,
    reset_pin: Output<'d>,
    fb: &'fb mut [u8],
) -> CO5300<'fb, EspQspi<'d>, Output<'d>> {
    let bus = display_hal::build_spi(spi, sclk, sio0, sio1, sio2, sio3, cs, dma);
    let mut display = CO5300::new(bus, reset_pin, fb);

    // Hardware reset: short low pulse then settle
    display.reset_high();
    Timer::after(Duration::from_millis(10)).await;
    display.reset_low();
    Timer::after(Duration::from_millis(10)).await;
    display.reset_high();
    Timer::after(Duration::from_millis(120)).await;

    log::info!("Display: initializing CO5300...");
    display.init().await;
    display.wake().await;
    Timer::after(Duration::from_millis(120)).await;
    display.display_on().await;
    Timer::after(Duration::from_millis(70)).await;
    log::info!("Display: ready");

    display
}
