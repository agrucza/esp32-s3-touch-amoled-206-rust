//! LittleFS-backed persistent storage.
//!
//! Mounts a LittleFS filesystem on a dedicated flash region and
//! exposes the two access patterns firmware needs:
//!
//! * **Versioned config blobs** - `load_blob<T>` / `save_blob<T>`
//!   for `Config` / `AlarmState` style value types, wrapped in
//!   `StoredBlob { version, inner }` and postcard-serialised.
//! * **Text log files** - `append_line` / `for_each_line` for the
//!   event log.
//!
//! ## Flash layout (per board)
//!
//! Addresses below **mirror** the `storage` partition declared in
//! each board's partition CSV. If you edit a CSV, update the
//! corresponding constants here - firmware doesn't read the
//! partition table at runtime (yet), so drift between the two
//! lands writes in the wrong region.
//!
//! | Board | Start         | Size      | Blocks | Partition CSV                                      |
//! |-------|---------------|-----------|--------|----------------------------------------------------|
//! | S3    | `0x0081_0000` | 23.875 MB | 6112   | `firmware/partitions-s3.csv`                       |
//! | C6    | TBD           | TBD       | TBD    | `firmware/partitions-c6.csv` (once C6 work starts) |
//!
//! Block size is the flash sector size (4 KB) on both boards.
//!
//! On S3 the last 64 KB (`0x01FF_0000..0x0200_0000`) is a
//! `coredump` partition. The factory firmware lives below us at
//! `0x0001_0000..0x0081_0000` (8 MB). The obsolete `system::nvs`
//! module used to park data at `0x00FB_0000`; after this
//! migration that region is no longer accessed by firmware code.
//!
//! ## Layout on the filesystem
//!
//! ```text
//! /system/
//! ├── config/
//! │   ├── config.bin   // postcard(StoredBlob<Config>)
//! │   └── alarms.bin   // postcard(StoredBlob<AlarmState>)
//! ├── logs/
//! │   └── events.log   // one CSV line per event
//! └── sounds/          // reserved for future alarm audio
//! ```
//!
//! The `/system/` prefix matches the SD-card layout, so the SD
//! mirror built in Stage C becomes a trivial byte-for-byte file
//! copy with no path translation. Future user-synced content
//! would land under `/user/` (not yet defined).

use alloc::vec::Vec;
use core::ops::ControlFlow;
use embedded_storage::nor_flash::{NorFlash, ReadNorFlash};
use esp_hal::peripherals::FLASH;
use esp_storage::{FlashStorage, FlashStorageError};
use littlefs_rust::{
    Config as LfsConfig, Error as LfsError, FileType, Filesystem, OpenFlags,
    Storage as LfsStorage,
};
use serde::{Deserialize, Serialize};

// -- Region geometry --------------------------------------------------------

// Board-specific region constants. Exactly one `board-*` Cargo
// feature must be active; the CSV at `firmware/partitions-<board>.csv`
// declares the matching `storage` partition.

#[cfg(not(any(feature = "board-s3", feature = "board-c6")))]
compile_error!("no board feature enabled - pick exactly one of board-s3 / board-c6");

#[cfg(all(feature = "board-s3", feature = "board-c6"))]
compile_error!("board-s3 and board-c6 are mutually exclusive");

#[cfg(feature = "board-c6")]
compile_error!("board-c6 partition layout not wired yet - add partitions-c6.csv and define FLASH_FS_START / FLASH_FS_SIZE");

/// Start of the LittleFS region, in bytes from the base of flash.
/// Sector-aligned (4 KB).
#[cfg(feature = "board-s3")]
pub const FLASH_FS_START: u32 = 0x0081_0000;

/// Size of the LittleFS region. On S3: 23.875 MB = 6112 erase blocks.
#[cfg(feature = "board-s3")]
pub const FLASH_FS_SIZE: u32 = 0x017E_0000;

/// End offset (exclusive).
pub const FLASH_FS_END: u32 = FLASH_FS_START + FLASH_FS_SIZE;

const BLOCK_SIZE:  u32 = 4096;
const BLOCK_COUNT: u32 = FLASH_FS_SIZE / BLOCK_SIZE;

// -- Storage adapter --------------------------------------------------------

/// Bridges `esp_storage::FlashStorage` (byte-addressable, NorFlash
/// trait) to `littlefs_rust::Storage` (block-addressable). Owns the
/// underlying `FlashStorage`.
pub struct FlashFsStorage<'d> {
    flash: FlashStorage<'d>,
}

impl<'d> FlashFsStorage<'d> {
    pub fn new(flash: FLASH<'d>) -> Self {
        Self { flash: FlashStorage::new(flash) }
    }
}

impl<'d> LfsStorage for FlashFsStorage<'d> {
    fn read(&mut self, block: u32, offset: u32, buf: &mut [u8]) -> Result<(), LfsError> {
        let addr = FLASH_FS_START + block * BLOCK_SIZE + offset;
        self.flash.read(addr, buf).map_err(map_storage_err)
    }

    fn write(&mut self, block: u32, offset: u32, data: &[u8]) -> Result<(), LfsError> {
        let addr = FLASH_FS_START + block * BLOCK_SIZE + offset;
        self.flash.write(addr, data).map_err(map_storage_err)
    }

    fn erase(&mut self, block: u32) -> Result<(), LfsError> {
        let from = FLASH_FS_START + block * BLOCK_SIZE;
        let to   = from + BLOCK_SIZE;
        self.flash.erase(from, to).map_err(map_storage_err)
    }
}

fn map_storage_err(e: FlashStorageError) -> LfsError {
    // Most FlashStorageError variants are I/O faults from the ROM
    // flash routines; `NotAligned` / `OutOfBounds` shouldn't happen
    // given our block geometry but map to `Invalid` for completeness.
    match e {
        FlashStorageError::NotAligned | FlashStorageError::OutOfBounds => LfsError::Invalid,
        _ => LfsError::Io,
    }
}

// -- Versioned blob wrapper -------------------------------------------------

/// Wrapper written to disk for any `save_blob` value. On load we
/// compare `version` to the caller's current build constant; a
/// mismatch causes the record to be dropped and the caller to fall
/// back to its default.
#[derive(Serialize, Deserialize)]
struct StoredBlob<T> {
    version: u8,
    inner: T,
}

// -- FlashFs ----------------------------------------------------------------

/// High-level access to the on-flash filesystem.
///
/// Thin wrapper around `littlefs_rust::Filesystem` that owns the
/// mount and exposes helpers the rest of the firmware uses. Created
/// once at boot by [`FlashFs::mount_or_format`].
pub struct FlashFs<'d> {
    fs: Filesystem<FlashFsStorage<'d>>,
    /// Postcard scratch buffer reused by `save_blob`. Large enough
    /// for the biggest versioned record we store (AlarmState fits
    /// in < 256 B; 512 is generous).
    buf: [u8; 512],
}

impl<'d> FlashFs<'d> {
    /// Mount the filesystem, formatting first if the mount fails.
    /// Blank flash / corrupted superblock / version mismatch all
    /// fall through the format path on first boot.
    ///
    /// Panics on format-then-mount failure; at that point the flash
    /// hardware itself is suspect and there's nothing we can do.
    pub fn mount_or_format(flash: FLASH<'d>) -> Self {
        let storage = FlashFsStorage::new(flash);
        let fs = match Filesystem::mount(storage, LfsConfig::new(BLOCK_SIZE, BLOCK_COUNT)) {
            Ok(fs) => {
                log::info!("flash_fs: mounted ({} blocks, {} KB)", BLOCK_COUNT, FLASH_FS_SIZE / 1024);
                fs
            }
            Err((e, mut storage)) => {
                log::warn!("flash_fs: mount failed ({:?}), formatting", e);
                Filesystem::format(&mut storage, &LfsConfig::new(BLOCK_SIZE, BLOCK_COUNT))
                    .expect("flash_fs: format failed");
                let fs = Filesystem::mount(storage, LfsConfig::new(BLOCK_SIZE, BLOCK_COUNT))
                    .map_err(|(e, _)| e)
                    .expect("flash_fs: mount after format failed");
                log::info!("flash_fs: formatted + mounted");
                fs
            }
        };
        // Ensure the firmware-owned directory tree exists. Each
        // `mkdir` returns `Exists` once the dir is there, so after
        // first boot these become cheap no-ops. Running them on
        // every mount (not just after format) keeps the layout
        // self-healing if an earlier build used a different layout.
        let _ = fs.mkdir("/system");
        let _ = fs.mkdir("/system/config");
        let _ = fs.mkdir("/system/logs");
        let _ = fs.mkdir("/system/sounds");
        Self { fs, buf: [0u8; 512] }
    }

    // -- Versioned blob helpers --------------------------------------------

    /// Read a postcard-serialised, version-tagged blob at `path`.
    ///
    /// Returns `None` if the file is missing, the record's version
    /// doesn't match `expected_version`, or deserialisation fails.
    /// Callers fall back to `T::default()` on `None`.
    pub fn load_blob<T>(&self, path: &str, expected_version: u8) -> Option<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let bytes = match self.fs.read_to_vec(path) {
            Ok(b) => b,
            Err(LfsError::NoEntry) => return None,
            Err(e) => {
                log::warn!("flash_fs: read {} failed: {:?}", path, e);
                return None;
            }
        };
        match postcard::from_bytes::<StoredBlob<T>>(&bytes) {
            Ok(stored) if stored.version == expected_version => Some(stored.inner),
            Ok(stored) => {
                log::warn!(
                    "flash_fs: {} version {} != expected {}; ignoring",
                    path, stored.version, expected_version,
                );
                None
            }
            Err(e) => {
                log::warn!("flash_fs: deserialising {} failed: {:?}", path, e);
                None
            }
        }
    }

    /// Write a postcard-serialised, version-tagged blob to `path`.
    /// Creates parent directories on demand.
    pub fn save_blob<T>(&mut self, path: &str, version: u8, value: &T)
    where
        T: Serialize,
    {
        let stored = StoredBlob { version, inner: value };
        let bytes = match postcard::to_slice(&stored, &mut self.buf) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("flash_fs: serialising {} failed: {:?}", path, e);
                return;
            }
        };
        if let Err(e) = self.fs.write_file(path, bytes) {
            log::warn!("flash_fs: write {} failed: {:?}", path, e);
        }
    }

    // -- Log file helpers --------------------------------------------------

    /// Append `bytes` to the file at `path`, creating it if missing.
    /// The caller is responsible for terminating lines with `\n`.
    pub fn append_line(&self, path: &str, bytes: &[u8]) -> Result<(), LfsError> {
        let file = self.fs.open(
            path,
            OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::APPEND,
        )?;
        let mut off = 0;
        while off < bytes.len() {
            let n = file.write(&bytes[off..])? as usize;
            if n == 0 {
                return Err(LfsError::Io);
            }
            off += n;
        }
        file.close()
    }

    /// Stream every line of the file at `path` through `callback`.
    ///
    /// The callback gets the line without its trailing newline. It
    /// may return `ControlFlow::Break(())` to stop the scan early.
    /// If the file doesn't exist the scan returns `Ok(0)` - that's
    /// a legitimate state (no events logged yet), not an error.
    pub fn for_each_line<F>(&self, path: &str, mut callback: F) -> Result<usize, LfsError>
    where
        F: FnMut(&str) -> ControlFlow<()>,
    {
        let file = match self.fs.open(path, OpenFlags::READ) {
            Ok(f) => f,
            Err(LfsError::NoEntry) => return Ok(0),
            Err(e) => return Err(e),
        };
        let mut io_buf = [0u8; 256];
        let mut line: heapless::Vec<u8, 96> = heapless::Vec::new();
        let mut visited = 0usize;
        let mut truncated = false;

        loop {
            let n = file.read(&mut io_buf)? as usize;
            if n == 0 {
                break;
            }
            for &b in &io_buf[..n] {
                if b == b'\n' {
                    if !truncated {
                        if let Ok(s) = core::str::from_utf8(&line) {
                            let s = s.strip_suffix('\r').unwrap_or(s);
                            if callback(s).is_break() {
                                let _ = file.close();
                                return Ok(visited + 1);
                            }
                        }
                    }
                    visited += 1;
                    line.clear();
                    truncated = false;
                } else if !truncated && line.push(b).is_err() {
                    truncated = true;
                    line.clear();
                }
            }
        }
        // Trailing partial line without newline.
        if !line.is_empty() && !truncated {
            if let Ok(s) = core::str::from_utf8(&line) {
                let _ = callback(s);
                visited += 1;
            }
        }
        file.close()?;
        Ok(visited)
    }

    /// Read the entire file at `path` into a `Vec`. Returns `None`
    /// on missing file; logs a warning and returns `None` on other
    /// I/O errors so callers can treat "read failed" uniformly.
    pub fn read_file(&self, path: &str) -> Option<Vec<u8>> {
        match self.fs.read_to_vec(path) {
            Ok(v) => Some(v),
            Err(LfsError::NoEntry) => None,
            Err(e) => {
                log::warn!("flash_fs: read {} failed: {:?}", path, e);
                None
            }
        }
    }

    /// Directories whose contents get deleted by [`Self::reset_user_data`].
    ///
    /// The policy: firmware-written data that should revert to its
    /// default state on factory reset goes here. User-authored
    /// content that a user would expect to survive (e.g. uploaded
    /// alarm sounds) deliberately does **not** go here.
    ///
    /// When you add a new persistence category, decide: does factory
    /// reset restore it to default (list it here) or preserve it
    /// (don't)?
    const FLASH_RESET_DIRS: &'static [&'static str] = &[
        "/system/config",
        "/system/logs",
    ];

    /// Delete every regular file inside every directory named in
    /// [`Self::FLASH_RESET_DIRS`]. The filesystem itself stays
    /// mounted; only listed content is wiped. Wired to the Storage
    /// settings "Factory reset" button.
    ///
    /// This is not a full reformat. If you need a true nuke, add a
    /// `reformat()` method that drives the filesystem through
    /// format + remount instead.
    pub fn reset_user_data(&self) {
        for dir in Self::FLASH_RESET_DIRS {
            let Ok(entries) = self.fs.list_dir(dir) else { continue };
            for entry in entries {
                let mut path: heapless::String<96> = heapless::String::new();
                if core::fmt::Write::write_fmt(&mut path, format_args!("{}/{}", dir, entry.name)).is_err() {
                    continue;
                }
                let _ = self.fs.remove(&path);
            }
        }
        log::info!("flash_fs: reset_user_data complete");
    }

    /// Approximate filesystem usage, expressed for the settings
    /// screen. `files` is the total number of regular files across
    /// the known directories, `total_bytes` is the region size.
    pub fn usage(&self) -> FsUsage {
        let mut files = 0u32;
        for dir in ["/system/config", "/system/logs", "/system/sounds"] {
            if let Ok(entries) = self.fs.list_dir(dir) {
                files += entries.iter().filter(|e| e.file_type == FileType::File).count() as u32;
            }
        }
        FsUsage { files, total_bytes: FLASH_FS_SIZE }
    }
}

/// Summary of filesystem usage, returned by [`FlashFs::usage`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FsUsage {
    pub files: u32,
    pub total_bytes: u32,
}
