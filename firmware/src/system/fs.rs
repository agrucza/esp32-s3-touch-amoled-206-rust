//! Storage-backend trait shared by `flash_fs::FlashFs` and
//! `sd_fs::SdFs`.
//!
//! The trait unifies the operations common to any filesystem-like
//! persistent backend: line-oriented append + scan, whole-file
//! read + write, and factory-reset wipe. Backend-specific concerns
//! (flash mount/format, SD probe, versioned blob helpers) stay on
//! the concrete types.
//!
//! Generic over backend means functions like "mirror a directory
//! from one place to another" can be written once and dispatched
//! to either side.

use alloc::vec::Vec;
use core::ops::ControlFlow;

/// A filesystem-like persistent backend.
///
/// No in-tree caller writes against the trait yet - it's the
/// abstraction a future file-sync / mirror / backup feature will
/// be written against. Impls exist on both backends now so that
/// generic work can be added without touching the concrete types.
#[allow(dead_code)]
///
/// Each method is best-effort and takes an absolute `path`. All
/// paths are expected to be rooted at `/system/...` on both
/// current backends (see `flash_fs.rs` and `sd_fs.rs` for the
/// on-disk layout).
///
/// The associated `Error` type lets each backend keep its native
/// error (littlefs vs. sdmmc) without an erasure layer. Consumers
/// typically either log the error or convert to `()`.
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
