//! SD card hardware bringup.
//!
//! Construction of the SPI peripheral, `SdCard`, and `VolumeManager`
//! happens here - those are the "cable" plumbing that's board-specific.
//! Everything above that (file ops, probe, reset) lives on
//! [`crate::system::sd_fs::SdFs`].

use crate::sdcard_hal;
use crate::system::sd_fs::SdFs;
use esp_hal::gpio::{Output, interconnect::{PeripheralInput, PeripheralOutput}};

/// Build the SD `VolumeManager` and wrap it in an [`SdFs`]. No I/O
/// yet - the underlying `SdCard` defers ACMD41 identification until
/// the first real read, so this succeeds whether or not a card is
/// in the slot. Call [`SdFs::probe`] afterwards to detect presence.
///
/// Returning `SdFs` directly (not `Option`) keeps the SPI peripheral
/// and CS pin alive for the whole boot; the runtime can re-probe on
/// demand (Settings "Initialize SD card" button) without re-plumbing
/// any hardware tokens.
pub fn init_sd<'d>(
    spi: impl esp_hal::spi::master::Instance + 'd,
    sck: impl PeripheralOutput<'d>,
    mosi: impl PeripheralOutput<'d>,
    miso: impl PeripheralInput<'d>,
    cs: Output<'d>,
) -> SdFs<'d> {
    log::info!("SD card: building volume manager (card access deferred)");
    let sd_card = sdcard_hal::build_sdcard(spi, sck, mosi, miso, cs);
    SdFs::new(drivers::sdcard::VolumeManager::new(sd_card, drivers::sdcard::RtcTimeSource))
}
