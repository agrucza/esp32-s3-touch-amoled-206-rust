use crate::sdcard_hal::{self, EspVolumeManager};
use drivers::sdcard::{BlockDevice as _, DummyTimeSource};
use esp_hal::gpio::{Output, interconnect::{PeripheralInput, PeripheralOutput}};

/// Initialize the SD card. Returns the volume manager or None if init failed.
pub fn init_sd<'d>(
    spi: impl esp_hal::spi::master::Instance + 'd,
    sck: impl PeripheralOutput<'d>,
    mosi: impl PeripheralOutput<'d>,
    miso: impl PeripheralInput<'d>,
    cs: Output<'d>,
) -> Option<EspVolumeManager<'d>> {
    log::info!("SD card: initializing...");
    let sd_card = sdcard_hal::build_sdcard(spi, sck, mosi, miso, cs);

    match sd_card.num_blocks() {
        Ok(n) => log::info!("SD card: {} blocks ({} MB)", n.0, n.0 as u64 * 512 / 1_000_000),
        Err(e) => {
            log::error!("SD card: not found or init failed: {:?}", e);
            return None;
        }
    }

    Some(EspVolumeManager::new(sd_card, DummyTimeSource))
}
