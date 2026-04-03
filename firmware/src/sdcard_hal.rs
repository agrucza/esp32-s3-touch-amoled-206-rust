//! ESP32-S3 HAL glue for the SD card SPI interface.
//!
//! Combines the SPI3 bus and the chip-select output pin into a single
//! `SpiDevice` (via `embedded-hal-bus::ExclusiveDevice`) and hands it to
//! `embedded-sdmmc::SdCard`.
//!
//! ## Pin assignments
//!
//! | Signal | GPIO |
//! |--------|------|
//! | MOSI   |  1   |
//! | SCK    |  2   |
//! | MISO   |  3   |
//! | CS     |  17  |
//!
//! SPI2 is reserved for the display; always use SPI3 here.

use drivers::sdcard::{DummyTimeSource, SdCard, VolumeManager};
use embedded_hal_bus::spi::{ExclusiveDevice, NoDelay};
use esp_hal::{
    Blocking,
    delay::Delay,
    gpio::{Output, interconnect::{PeripheralInput, PeripheralOutput}},
    spi::master::{Config, Spi},
    spi::Mode,
    time::Rate,
};

/// Concrete SdCard type returned by [`build_sdcard`].
pub type EspSdCard<'d> = SdCard<
    ExclusiveDevice<Spi<'d, Blocking>, Output<'d>, NoDelay>,
    Delay,
>;

/// Convenience alias: VolumeManager backed by the ESP SdCard + DummyTimeSource.
pub type EspVolumeManager<'d> = VolumeManager<EspSdCard<'d>, DummyTimeSource>;

/// Build an [`EspSdCard`] from raw esp-hal peripherals.
///
/// The SPI bus is initialised at 400 kHz (safe for card identification).
/// After init, call `SdCard::get_card_type()` to confirm detection, then
/// raise the frequency via the underlying bus if needed.
///
/// # Parameters
/// - `spi`  - SPI peripheral (use `p.SPI3`; SPI2 is taken by the display)
/// - `sck`  - clock pin (GPIO2)
/// - `mosi` - data out to card (GPIO1)
/// - `miso` - data in from card (GPIO3)
/// - `cs`   - chip select output, already initialised high (GPIO17)
pub fn build_sdcard<'d>(
    spi:  impl esp_hal::spi::master::Instance + 'd,
    sck:  impl PeripheralOutput<'d>,
    mosi: impl PeripheralOutput<'d>,
    miso: impl PeripheralInput<'d>,
    cs:   Output<'d>,
) -> EspSdCard<'d> {
    let spi_bus = Spi::new(
        spi,
        Config::default()
            .with_frequency(Rate::from_khz(400))
            .with_mode(Mode::_0),
    )
    .unwrap()
    .with_sck(sck)
    .with_mosi(mosi)
    .with_miso(miso);

    // ExclusiveDevice wraps the bus + CS into a SpiDevice.
    // new_no_delay: no inter-transaction CS gap needed on a dedicated bus.
    // new_no_delay: stores NoDelay sentinel - no inter-transaction gap needed
    // on a dedicated bus. The SdCard itself gets a separate Delay for protocol timing.
    let spi_device = ExclusiveDevice::new_no_delay(spi_bus, cs).unwrap();

    SdCard::new(spi_device, Delay::new())
}
