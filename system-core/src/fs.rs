//! Filesystem-backend trait and shared versioned-blob helpers.
//!
//! Two things live here:
//!
//! * The [`Storage`] trait, implemented by the filesystem backends
//!   ([`crate::flash_fs::FlashFs`] and the SD backend). Unifies the
//!   line-oriented + whole-file operations common to any
//!   filesystem-like persistent backend so generic code (e.g. the
//!   `Store` composite's "wipe every backend" path) can dispatch
//!   uniformly across the backends.
//!
//! * A small set of versioned-blob helpers ([`StoredBlob`],
//!   [`wrap_blob`], [`unwrap_blob`]). Shared so the flash primary
//!   writer and the mirror writer encode config / alarm records
//!   identically - mirrored blob files are byte-for-byte copies of
//!   their flash peers.
//!
//! Part of the storage subsystem in `system-core`; board-agnostic.

use alloc::vec::Vec;
use core::ops::ControlFlow;
use serde::{Deserialize, Serialize};

// -- Storage trait ----------------------------------------------------------

/// A filesystem-like persistent backend.
///
/// Each method is best-effort and takes an absolute `path`. All
/// paths are expected to be rooted at `/system/...` on every backend
/// (see `flash_fs.rs` / `sd_fs.rs` for the on-disk layout).
///
/// The associated `Error` type lets each backend keep its native
/// error (littlefs vs. sdmmc) without an erasure layer. Consumers
/// typically either log the error or convert to `()`.
///
/// ### Why the trait exists
///
/// The `Store` composite doesn't dispatch through `&mut dyn Storage`
/// today - it calls `self.flash.<op>()` / `self.sd.<op>()` directly,
/// because the two backends compose in non-identical ways (SD writes
/// are gated by the online flag; flash is authoritative). The trait's
/// job is to enforce that the backends have matching shapes, so the
/// "mirror op" patterns stay symmetrical and a flash-only
/// configuration can be supported without widening the surface.
#[allow(dead_code)] // contract enforcer, not a dispatch point
pub trait Storage {
    /// Native error type for this backend.
    type Error: core::fmt::Debug;

    /// Append `bytes` to the file at `path`. Creates parent
    /// directories and the file itself as needed.
    fn append_line(&mut self, path: &str, bytes: &[u8])
        -> Result<(), Self::Error>;

    /// Stream every line of the file at `path` through `callback`.
    /// Returns the number of lines visited, or `Ok(0)` if the file
    /// is absent (a legitimate "nothing logged yet" state, not an
    /// error).
    ///
    /// The callback receives each line without its trailing
    /// newline and may return `ControlFlow::Break(())` to stop the
    /// scan early.
    fn for_each_line<F>(&mut self, path: &str, callback: F)
        -> Result<usize, Self::Error>
    where
        F: FnMut(&str) -> ControlFlow<()>;

    /// Read the entire file at `path`. Returns `None` on missing
    /// file; warns-and-returns-`None` on other I/O errors so
    /// callers can treat "read failed" uniformly.
    fn read_file(&mut self, path: &str) -> Option<Vec<u8>>;

    /// Write `bytes` to `path`, truncating if the file already
    /// exists. Creates parent directories as needed.
    fn write_file(&mut self, path: &str, bytes: &[u8])
        -> Result<(), Self::Error>;

    /// Wipe the backend's user-data set. Each backend defines its
    /// own reach (`FlashFs` clears `FLASH_RESET_DIRS`, `SdFs`
    /// clears `SD_RESET_CHILDREN`). Directory structure stays in
    /// place; only files are removed.
    fn reset_user_data(&mut self);
}

// -- Versioned blob helpers -------------------------------------------------

/// Envelope written to disk for any versioned blob (config,
/// alarms). On load we compare `version` to the caller's current
/// build constant; a mismatch causes the record to be dropped and
/// the caller to fall back to its default.
///
/// Kept in this module (rather than on `FlashFs`) so that both the
/// flash write and the SD mirror write emit the same byte stream.
#[derive(Serialize, Deserialize)]
pub(crate) struct StoredBlob<T> {
    pub version: u8,
    pub inner: T,
}

/// Serialise `{ version, value }` into `buf`. Returns the populated
/// prefix on success, or `None` if `buf` is too small. Callers size
/// the buffer based on the biggest blob they persist (512 B is
/// generous for today's `Config` / `AlarmState`).
pub fn wrap_blob<'b, T: Serialize>(
    buf: &'b mut [u8],
    version: u8,
    value: &T,
) -> Option<&'b [u8]> {
    match postcard::to_slice(&StoredBlob { version, inner: value }, buf) {
        Ok(slice) => Some(slice),
        Err(e) => {
            log::warn!("fs: blob serialise failed: {:?}", e);
            None
        }
    }
}

/// Deserialise a versioned blob from `bytes`. Returns the inner
/// value if the envelope's `version` matches `expected_version`;
/// otherwise logs a warning and returns `None`.
pub fn unwrap_blob<T>(bytes: &[u8], expected_version: u8) -> Option<T>
where
    T: for<'de> Deserialize<'de>,
{
    match postcard::from_bytes::<StoredBlob<T>>(bytes) {
        Ok(stored) if stored.version == expected_version => Some(stored.inner),
        Ok(stored) => {
            log::warn!(
                "fs: blob version {} != expected {}; ignoring",
                stored.version, expected_version,
            );
            None
        }
        Err(e) => {
            log::warn!("fs: blob deserialise failed: {:?}", e);
            None
        }
    }
}
