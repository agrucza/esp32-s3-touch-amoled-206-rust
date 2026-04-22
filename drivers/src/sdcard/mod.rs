//! SD card driver module.
//!
//! Re-exports `embedded-sdmmc` types needed for block-level and filesystem
//! access, plus two `TimeSource` implementations:
//!
//!   * [`RtcTimeSource`] - zero-sized reader of a shared wall clock
//!     that firmware updates from the RTC. Use this in production so
//!     file timestamps match reality.
//!   * [`DummyTimeSource`] - fixed stub kept for development and for
//!     any path that wants a TimeSource without the RTC bridge.
//!
//! ## Architecture
//!
//! This crate is HAL-agnostic; it only depends on `embedded-hal` traits.
//! The ESP-specific SPI wiring (creating a `SpiDevice` from a `SpiBus` +
//! chip-select pin via `embedded-hal-bus`) lives in the firmware crate at
//! `firmware/src/sdcard_hal.rs`, following the same pattern as `display_hal.rs`.
//!
//! ## Wall-clock bridge
//!
//! `TimeSource::get_timestamp(&self)` is synchronous and non-fallible,
//! so it can't do I²C to the PCF85063 itself. Instead, firmware calls
//! [`update_wall_clock`] whenever fresh RTC data arrives; readers
//! (here and anywhere else that wants the cached calendar time) go
//! through a pair of `AtomicU32`s (date + time), which is lock-free
//! and safe across tasks / interrupts. Xtensa / ESP32-S3 has no
//! native 64-bit atomic ops, so two 32-bit atomics beat a
//! critical-section-emulated `AtomicU64` for this hot read path.
//!
//! Tearing risk: a reader can observe `DATE` from tick N and
//! `TIME` from tick N+1 (1 s later). For file-mtimes that's
//! unnoticeable, so we pay no synchronisation cost.

use core::sync::atomic::{AtomicU32, Ordering};

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

// -- Shared wall clock ------------------------------------------------------

/// Packed calendar DATE shared between the RTC and any
/// [`TimeSource`] consumer. Layout (LSB to MSB):
///
/// | bits  | field                            |
/// |-------|----------------------------------|
/// |  0..5 | zero-indexed day (0-30)          |
/// |  5..9 | zero-indexed month (0-11)        |
/// |  9..17| year since 1970 (0-254)          |
///
/// `0` means "never written" (would decode to 1970-01-01, which the
/// PCF85063 cannot produce since its range starts in the 2000s).
/// Readers treat that sentinel as the FAT epoch.
static WALL_CLOCK_DATE: AtomicU32 = AtomicU32::new(0);

/// Packed wall-clock TIME. Layout:
///
/// | bits  | field             |
/// |-------|-------------------|
/// |  0..6 | seconds (0-59)    |
/// |  6..12| minutes (0-59)    |
/// | 12..17| hours (0-23)      |
static WALL_CLOCK_TIME: AtomicU32 = AtomicU32::new(0);

/// Update the shared wall clock read by [`RtcTimeSource`]. Call
/// this whenever the RTC reports a fresh time.
///
/// Date and time update as two separate `AtomicU32` stores. A
/// reader that catches between the two sees a ~1 s stale value -
/// acceptable for file mtimes and log-line timestamps.
pub fn update_wall_clock(year: u16, month: u8, day: u8, hour: u8, minute: u8, second: u8) {
    let y = year.saturating_sub(1970).min(254) as u32;
    let m = month.saturating_sub(1).min(11) as u32;
    let d = day.saturating_sub(1).min(30) as u32;
    let date = (y << 9) | (m << 5) | d;
    let time =
          ((hour   as u32 & 0x1F) << 12)
        | ((minute as u32 & 0x3F) <<  6)
        |  (second as u32 & 0x3F);
    // Store TIME first so the (rare) torn read shows the newer
    // TIME paired with the newer DATE once DATE lands.
    WALL_CLOCK_TIME.store(time, Ordering::Relaxed);
    WALL_CLOCK_DATE.store(date, Ordering::Relaxed);
}

/// [`TimeSource`] backed by the shared wall clock. Updates come from
/// [`update_wall_clock`] calls on the firmware side; this type is
/// zero-sized and cheap to copy, so every `VolumeManager` can own one
/// without caring about lifetimes.
///
/// Before the first update, `get_timestamp` returns the FAT epoch
/// (1980-01-01 00:00:00) so files created early in boot still have
/// valid timestamps.
pub struct RtcTimeSource;

impl TimeSource for RtcTimeSource {
    fn get_timestamp(&self) -> Timestamp {
        let date = WALL_CLOCK_DATE.load(Ordering::Relaxed);
        let time = WALL_CLOCK_TIME.load(Ordering::Relaxed);
        if date == 0 {
            return Timestamp {
                year_since_1970: 10, // 1980 = FAT epoch
                zero_indexed_month: 0,
                zero_indexed_day: 0,
                hours: 0, minutes: 0, seconds: 0,
            };
        }
        Timestamp {
            year_since_1970:    ((date >>  9) & 0xFF) as u8,
            zero_indexed_month: ((date >>  5) & 0x0F) as u8,
            zero_indexed_day:   ( date        & 0x1F) as u8,
            hours:              ((time >> 12) & 0x1F) as u8,
            minutes:            ((time >>  6) & 0x3F) as u8,
            seconds:            ( time        & 0x3F) as u8,
        }
    }
}

/// Dummy [`TimeSource`] that always returns a fixed timestamp.
///
/// Kept for development, host-side tests, and any path that wants a
/// TimeSource without the RTC bridge. Production firmware uses
/// [`RtcTimeSource`].
#[allow(dead_code)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wall_clock_round_trip() {
        update_wall_clock(2026, 4, 22, 19, 3, 14);
        let ts = RtcTimeSource.get_timestamp();
        assert_eq!(ts.year_since_1970, 56);
        assert_eq!(ts.zero_indexed_month, 3);
        assert_eq!(ts.zero_indexed_day, 21);
        assert_eq!(ts.hours, 19);
        assert_eq!(ts.minutes, 3);
        assert_eq!(ts.seconds, 14);
    }

    #[test]
    fn wall_clock_clamps_prehistoric_years() {
        update_wall_clock(1900, 1, 1, 0, 0, 0);
        let ts = RtcTimeSource.get_timestamp();
        // 1900 clamps to 1970; 1970-01-01 packs to 0, which the
        // reader intentionally treats as "never written" and
        // returns the FAT epoch (1980).
        assert_eq!(ts.year_since_1970, 10);
    }
}
