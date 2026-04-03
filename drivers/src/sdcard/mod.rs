//! SD card driver module.
//!
//! Re-exports `embedded-sdmmc` types needed for block-level and filesystem
//! access, plus a `DummyTimeSource` for development use.
//!
//! ## Architecture
//!
//! This crate is HAL-agnostic; it only depends on `embedded-hal` traits.
//! The ESP-specific SPI wiring (creating a `SpiDevice` from a `SpiBus` +
//! chip-select pin via `embedded-hal-bus`) lives in the firmware crate at
//! `firmware/src/sdcard_hal.rs`, following the same pattern as `display_hal.rs`.
//!
//! ## Usage sketch
//!
//! ```ignore
//! // firmware/src/sdcard_hal.rs handles this:
//! let spi_device = ExclusiveDevice::new_no_delay(spi_bus, cs_pin).unwrap();
//! let sdcard    = SdCard::new(spi_device, delay);
//! let mut vol   = VolumeManager::new(sdcard, DummyTimeSource);
//! ```

pub use embedded_sdmmc::{
    Block, BlockCount, BlockDevice, BlockIdx,
    DirEntry,
    Error as SdmmcError,
    Mode as FileMode,
    RawDirectory, RawFile, RawVolume,
    SdCard, SdCardError,
    TimeSource, Timestamp,
    VolumeIdx,
    VolumeManager,
};

/// Dummy [`TimeSource`] that always returns a fixed timestamp.
///
/// Sufficient for development and testing. Replace with an RTC-backed
/// implementation when accurate file creation/modification times matter.
pub struct DummyTimeSource;

impl TimeSource for DummyTimeSource {
    fn get_timestamp(&self) -> Timestamp {
        Timestamp {
            year_since_1970: 56, // 2026 - 1970
            zero_indexed_month: 3, // April (0 = January)
            zero_indexed_day: 2,   // 3rd (0-indexed)
            hours: 0,
            minutes: 0,
            seconds: 0,
        }
    }
}
