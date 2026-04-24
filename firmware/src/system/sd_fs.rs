//! SD card filesystem wrapper.
//!
//! Wraps `embedded-sdmmc::VolumeManager` behind an API symmetric
//! with `system::flash_fs::FlashFs`: `append_line`, `for_each_line`,
//! `reset_user_data`, etc. Consumers (event_log, FactoryReset
//! handler) talk to `SdFs` without touching FAT-specific
//! volume/directory/file mechanics.
//!
//! ## Layout on the card
//!
//! ```text
//! /system/
//! └── logs/
//!     └── events.log     // mirror of the flash log, identical format
//! ```
//!
//! User content anywhere else on the card is never touched.

use crate::sdcard_hal::EspVolumeManager;
use alloc::vec::Vec;
use app_core::log::{LogEntry, parse_log_line};
use core::ops::ControlFlow;
use drivers::sdcard::{
    DirEntry, FileMode, RawDirectory, RawFile, SdCardError, SdmmcError, VolumeIdx,
};

/// Directories under `/system/` that [`SdFs::reset_user_data`]
/// wipes. Narrower than the flash-side reset - SD is user-visible
/// removable media, so we only touch what we authored.
const RESET_CHILDREN: &[&str] = &["logs"];

/// Top-level `/system/` directory name.
const SYSTEM_DIR: &str = "system";

/// Max nesting depth supported by the path walker. 4 is plenty for
/// `/system/logs/events.log` (2 dirs + file) with headroom.
const MAX_DEPTH: usize = 4;

/// Max line length accepted by [`SdFs::for_each_line`]. Anything
/// longer gets truncated. The writer never emits lines longer than
/// ~50 bytes today; 96 covers that plus future detail-column growth.
const READ_LINE_CAP: usize = 96;

/// SD card filesystem handle.
///
/// Owns the `embedded-sdmmc::VolumeManager`. Construction succeeds
/// even without a card in the slot - call [`Self::probe`] afterwards
/// to detect presence. All file ops are best-effort; the caller
/// (typically via the composite `Store`'s online flag) decides
/// whether to attempt them.
pub struct SdFs<'d> {
    vol: EspVolumeManager<'d>,
}

impl<'d> SdFs<'d> {
    pub fn new(vol: EspVolumeManager<'d>) -> Self {
        Self { vol }
    }

    /// Open + close volume 0. Returns `true` if a card is present
    /// and the FAT MBR is readable.
    pub fn probe(&mut self) -> bool {
        match self.vol.open_raw_volume(VolumeIdx(0)) {
            Ok(vol) => {
                let _ = self.vol.close_volume(vol);
                log::info!("SD card: present (volume 0 readable)");
                true
            }
            Err(e) => {
                log::info!("SD card: absent or unreadable ({:?})", e);
                false
            }
        }
    }

    /// Append `bytes` to the file at `path`, creating parent
    /// directories as needed. `path` must be absolute.
    pub fn append_line(&mut self, path: &str, bytes: &[u8])
        -> Result<(), SdmmcError<SdCardError>>
    {
        self.with_file(path, FileMode::ReadWriteCreateOrAppend, true, |vol, file| {
            let result = vol.write(file, bytes);
            let _ = vol.flush_file(file);
            result
        })
    }

    /// Stream every line of `path` through `callback`. Returns the
    /// number of lines visited, or `Ok(0)` if the file is absent
    /// (a fresh card with nothing written yet is a legitimate
    /// state, not an error).
    ///
    /// The callback receives each line as `&str` without its
    /// trailing newline. Return `ControlFlow::Break(())` to stop
    /// the scan early.
    pub fn for_each_line<F>(&mut self, path: &str, mut callback: F)
        -> Result<usize, SdmmcError<SdCardError>>
    where
        F: FnMut(&str) -> ControlFlow<()>,
    {
        let result = self.with_file(path, FileMode::ReadOnly, false, |vol, file| {
            let mut io_buf = [0u8; 256];
            let mut line_buf: heapless::Vec<u8, READ_LINE_CAP> = heapless::Vec::new();
            let mut visited = 0usize;
            let mut dropped = false;

            loop {
                let n = vol.read(file, &mut io_buf)?;
                if n == 0 { break; }
                for &b in &io_buf[..n] {
                    if b == b'\n' {
                        if !dropped {
                            if let Ok(s) = core::str::from_utf8(&line_buf) {
                                let s = s.strip_suffix('\r').unwrap_or(s);
                                if callback(s).is_break() {
                                    return Ok(visited + 1);
                                }
                            }
                        }
                        visited += 1;
                        line_buf.clear();
                        dropped = false;
                    } else if !dropped && line_buf.push(b).is_err() {
                        dropped = true;
                        line_buf.clear();
                    }
                }
            }
            if !line_buf.is_empty() && !dropped {
                if let Ok(s) = core::str::from_utf8(&line_buf) {
                    let _ = callback(s);
                    visited += 1;
                }
            }
            Ok(visited)
        });

        match result {
            Ok(n) => Ok(n),
            Err(SdmmcError::NotFound) => Ok(0),
            Err(e) => Err(e),
        }
    }

    /// Read the entire file at `path` into a `Vec`. Returns
    /// `None` on missing file; warns-and-returns-`None` on other
    /// I/O errors so callers can treat "read failed" uniformly.
    pub fn read_file(&mut self, path: &str) -> Option<Vec<u8>> {
        let result = self.with_file(path, FileMode::ReadOnly, false, |vol, file| {
            let len = vol.file_length(file)? as usize;
            let mut buf: Vec<u8> = alloc::vec![0u8; len];
            let mut off = 0;
            while off < len {
                let n = vol.read(file, &mut buf[off..])?;
                if n == 0 { break; }
                off += n;
            }
            buf.truncate(off);
            Ok(buf)
        });
        match result {
            Ok(buf) => Some(buf),
            Err(SdmmcError::NotFound) => None,
            Err(e) => {
                log::warn!("sd_fs: read {} failed: {:?}", path, e);
                None
            }
        }
    }

    /// Write `bytes` to `path`, creating-or-truncating the file.
    /// Creates parent directories as needed.
    ///
    /// Used by `Store::save_blob` for the SD-mirror side; the flash
    /// write goes through `FlashFs::save_blob` directly.
    pub fn write_file(&mut self, path: &str, bytes: &[u8])
        -> Result<(), SdmmcError<SdCardError>>
    {
        self.with_file(path, FileMode::ReadWriteCreateOrTruncate, true, |vol, file| {
            vol.write(file, bytes)?;
            vol.flush_file(file)
        })
    }

    /// Read a page of parsed log entries starting at `start_line`.
    /// Intended for a future text-viewer screen.
    #[allow(dead_code)] // wired once the event-log viewer screen lands
    pub fn read_page(&mut self, path: &str, start_line: usize, out: &mut [LogEntry])
        -> Result<usize, SdmmcError<SdCardError>>
    {
        let mut skipped = 0usize;
        let mut written = 0usize;
        self.for_each_line(path, |line| {
            if skipped < start_line { skipped += 1; return ControlFlow::Continue(()); }
            if written >= out.len() { return ControlFlow::Break(()); }
            if let Some(entry) = parse_log_line(line) {
                out[written] = entry;
                written += 1;
            }
            ControlFlow::Continue(())
        })?;
        Ok(written)
    }

    /// Ring-buffer the last `out.len()` entries matching `tag` into
    /// `out`, oldest-first. Intended for the battery-history chart
    /// and similar bounded-history screens.
    #[allow(dead_code)] // wired once the battery-history screen lands
    pub fn read_recent_by_tag(&mut self, path: &str, tag: &str, out: &mut [LogEntry])
        -> Result<usize, SdmmcError<SdCardError>>
    {
        if out.is_empty() { return Ok(0); }
        let mut head = 0usize;
        let mut filled = 0usize;

        self.for_each_line(path, |line| {
            if let Some(entry) = parse_log_line(line) {
                if entry.tag.as_str() == tag {
                    out[head] = entry;
                    head = (head + 1) % out.len();
                    if filled < out.len() { filled += 1; }
                }
            }
            ControlFlow::Continue(())
        })?;

        if filled < out.len() { return Ok(filled); }
        out.rotate_left(head);
        Ok(filled)
    }

    /// Delete every regular file inside `/system/<child>/` for each
    /// child in [`RESET_CHILDREN`]. Directories themselves stay put;
    /// user content elsewhere on the card is preserved.
    pub fn reset_user_data(&mut self) {
        match self.walk_reset() {
            Ok(n) => log::info!("SD card: reset complete, {} files removed", n),
            Err(e) => log::warn!("SD card: reset errored ({:?})", e),
        }
    }

    // -- Internals ----------------------------------------------------------

    /// Open `path` for reading/writing with the given mode, walking
    /// and (optionally) creating parent directories. Runs `f` with
    /// the open file handle; guarantees every opened handle is
    /// closed on every exit path.
    fn with_file<T, F>(
        &mut self,
        path: &str,
        mode: FileMode,
        create_dirs: bool,
        f: F,
    ) -> Result<T, SdmmcError<SdCardError>>
    where
        F: FnOnce(&mut EspVolumeManager<'d>, RawFile) -> Result<T, SdmmcError<SdCardError>>,
    {
        // Split "/system/logs/events.log" into dir components +
        // filename. Leading slash → first component is empty, skip.
        let mut parts = path.trim_start_matches('/').split('/');
        let Some(first) = parts.next() else { return Err(SdmmcError::NotFound) };
        let mut components: heapless::Vec<&str, MAX_DEPTH> = heapless::Vec::new();
        let _ = components.push(first);
        for p in parts {
            if components.push(p).is_err() {
                return Err(SdmmcError::NotFound); // path deeper than MAX_DEPTH
            }
        }
        let Some(filename) = components.pop() else { return Err(SdmmcError::NotFound) };

        let vol_handle = self.vol.open_raw_volume(VolumeIdx(0))?;
        let root = match self.vol.open_root_dir(vol_handle) {
            Ok(d) => d,
            Err(e) => {
                let _ = self.vol.close_volume(vol_handle);
                return Err(e);
            }
        };

        // Walk directories. Stash each handle so we can close in
        // reverse order regardless of which step fails.
        let mut dirs: heapless::Vec<RawDirectory, MAX_DEPTH> = heapless::Vec::new();
        let mut parent = root;
        for name in &components {
            match open_or_create_dir(&mut self.vol, parent, name, create_dirs) {
                Ok(d) => {
                    parent = d;
                    let _ = dirs.push(d);
                }
                Err(e) => {
                    close_all(&mut self.vol, &dirs, root, vol_handle);
                    return Err(e);
                }
            }
        }

        let file = match self.vol.open_file_in_dir(parent, filename, mode) {
            Ok(f) => f,
            Err(e) => {
                close_all(&mut self.vol, &dirs, root, vol_handle);
                return Err(e);
            }
        };

        let result = f(&mut self.vol, file);

        let _ = self.vol.close_file(file);
        close_all(&mut self.vol, &dirs, root, vol_handle);
        result
    }

    fn walk_reset(&mut self) -> Result<u32, SdmmcError<SdCardError>> {
        let vol_handle = self.vol.open_raw_volume(VolumeIdx(0))?;
        let root = match self.vol.open_root_dir(vol_handle) {
            Ok(d) => d,
            Err(e) => {
                let _ = self.vol.close_volume(vol_handle);
                return Err(e);
            }
        };
        let sysdir = match self.vol.open_dir(root, SYSTEM_DIR) {
            Ok(d) => d,
            Err(SdmmcError::NotFound) => {
                // Card has no /system/ - nothing to wipe.
                let _ = self.vol.close_dir(root);
                let _ = self.vol.close_volume(vol_handle);
                return Ok(0);
            }
            Err(e) => {
                let _ = self.vol.close_dir(root);
                let _ = self.vol.close_volume(vol_handle);
                return Err(e);
            }
        };

        let mut removed = 0u32;
        for child in RESET_CHILDREN {
            let child_dir = match self.vol.open_dir(sysdir, *child) {
                Ok(d) => d,
                Err(SdmmcError::NotFound) => continue,
                Err(e) => {
                    log::warn!("SD reset: open /system/{} failed: {:?}", child, e);
                    continue;
                }
            };

            // Collect names first - can't delete during iterate_dir.
            let mut to_delete: heapless::Vec<DirEntry, 32> = heapless::Vec::new();
            let _ = self.vol.iterate_dir(child_dir, |entry: &DirEntry| {
                if entry.attributes.is_directory() { return; }
                let _ = to_delete.push(entry.clone());
            });

            for entry in &to_delete {
                match self.vol.delete_file_in_dir(child_dir, entry.name.clone()) {
                    Ok(()) => removed += 1,
                    Err(e) => log::warn!(
                        "SD reset: delete /system/{}/{:?} failed: {:?}",
                        child, entry.name, e,
                    ),
                }
            }

            let _ = self.vol.close_dir(child_dir);
        }

        let _ = self.vol.close_dir(sysdir);
        let _ = self.vol.close_dir(root);
        let _ = self.vol.close_volume(vol_handle);
        Ok(removed)
    }
}

/// Open `name` inside `parent`. If `NotFound` and `create` is true,
/// create and retry. Any other error (or NotFound with create=false)
/// is returned as-is.
fn open_or_create_dir(
    vol: &mut EspVolumeManager,
    parent: RawDirectory,
    name: &str,
    create: bool,
) -> Result<RawDirectory, SdmmcError<SdCardError>> {
    match vol.open_dir(parent, name) {
        Ok(d) => Ok(d),
        Err(SdmmcError::NotFound) if create => {
            vol.make_dir_in_dir(parent, name)?;
            vol.open_dir(parent, name)
        }
        Err(e) => Err(e),
    }
}

/// Close every directory handle in `dirs` (reverse order) plus the
/// root directory and volume. Errors during cleanup are swallowed
/// since we're already unwinding.
fn close_all(
    vol: &mut EspVolumeManager,
    dirs: &heapless::Vec<RawDirectory, MAX_DEPTH>,
    root: RawDirectory,
    vol_handle: drivers::sdcard::RawVolume,
) {
    for d in dirs.iter().rev() {
        let _ = vol.close_dir(*d);
    }
    let _ = vol.close_dir(root);
    let _ = vol.close_volume(vol_handle);
}

// -- Storage trait impl -----------------------------------------------------

impl<'d> crate::system::fs::Storage for SdFs<'d> {
    type Error = SdmmcError<SdCardError>;

    fn append_line(&mut self, path: &str, bytes: &[u8]) -> Result<(), Self::Error> {
        self.append_line(path, bytes)
    }

    fn for_each_line<F>(&mut self, path: &str, callback: F) -> Result<usize, Self::Error>
    where
        F: FnMut(&str) -> ControlFlow<()>,
    {
        self.for_each_line(path, callback)
    }

    fn read_file(&mut self, path: &str) -> Option<Vec<u8>> {
        self.read_file(path)
    }

    fn write_file(&mut self, path: &str, bytes: &[u8]) -> Result<(), Self::Error> {
        self.write_file(path, bytes)
    }

    fn reset_user_data(&mut self) {
        self.reset_user_data();
    }
}
