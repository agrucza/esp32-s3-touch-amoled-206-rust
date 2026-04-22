//! Event log (flash + optional SD mirror).
//!
//! Appends a CSV line for each loggable [`SystemEvent`] to the
//! on-flash event log (always, via [`crate::system::flash_fs`]) and
//! also to the SD card if present. Both sides use an identical
//! text format, so the SD mirror is a straight file-append - no
//! translation step.
//!
//! ## Format
//!
//! ```text
//! <seq>,YYYY-MM-DDTHH:MM:SS,<tag>[,<detail>]
//! ```
//!
//! * `<seq>` is a flash-persistent monotonic `u32`. Recovered at
//!   boot by scanning the flash log for the highest seen value +
//!   1. Drives the Stage C SD back-fill ("copy entries with
//!   `seq > last_mirrored`").
//! * Timestamp is local wall time from the PCF85063 (no timezone).
//! * `<tag>` is the static string from [`LoggedEvent`].
//! * `<detail>`, when present, is a single integer (e.g. battery percent).
//!
//! ## Layout on disk
//!
//! Identical on both sides: `/system/logs/events.log`. Created
//! on first write. The `/system/` prefix keeps firmware state
//! separate from any user content on the SD card, and mirrors the
//! same layout on flash so the Stage C mirror is a byte-for-byte
//! file copy with no path translation.
//!
//! ## I/O strategy
//!
//! Events are rare (alarms, timer expiries, battery-percent changes)
//! so we open-append-close the file on each side for every line.
//! That keeps both storage handles easy to share with other code
//! without lifetime gymnastics, at the cost of a few extra protocol
//! round-trips per write. Fine at this access rate.
//!
//! ## Failure handling
//!
//! Every call is best-effort. Flash or SD write failures get a
//! single `log::warn!` and continue. The caller doesn't care whether
//! either side succeeded.

use crate::sdcard_hal::EspVolumeManager;
use crate::system::flash_fs::FlashFs;
use app_core::data::TimeData;
use app_core::events::{LoggedEvent, SystemEvent, classify_for_log};
use app_core::log::{LogEntry, parse_log_line};
use core::fmt::Write as _;
use core::ops::ControlFlow;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use drivers::sdcard::{FileMode, RawDirectory, RawFile, SdCardError, SdmmcError, VolumeIdx};

const FLASH_LOG_PATH: &str = "/system/logs/events.log";
// SD mirror lives at /system/logs/events.log. embedded-sdmmc's
// volume manager operates one directory level at a time, so the
// path is represented as two components here and walked in
// `append_bytes`.
const SD_SYSTEM_DIR: &str = "system";
const SD_LOGS_DIR:   &str = "logs";
const SD_LOG_FILENAME: &str = "events.log";

/// Next sequence number to emit. Initialised at boot by
/// [`init_seq_from_flash`] (which scans `/system/logs/events.log` and sets
/// this to `max_seq + 1`). Incremented on every append.
static NEXT_SEQ: AtomicU32 = AtomicU32::new(1);

/// Whether the SD card is currently usable for mirror writes.
///
/// * Set `true` by the manager after `storage::probe_sd` succeeds
///   (at boot and on user-triggered re-init).
/// * Flipped `false` automatically on the first SD write failure -
///   stops warn-spam if the card got pulled mid-session. User has
///   to tap "Initialize SD card" in Settings to bring it back.
///
/// Flash writes are unaffected by this flag; the flash side is
/// authoritative.
static SD_ONLINE: AtomicBool = AtomicBool::new(false);

/// Mark the SD mirror online (typically after a successful probe).
pub fn set_sd_online(online: bool) {
    SD_ONLINE.store(online, Ordering::Relaxed);
}

/// Current SD mirror state. Used by the Settings UI to render the
/// status line.
pub fn sd_online() -> bool {
    SD_ONLINE.load(Ordering::Relaxed)
}

/// Copy any flash log entry whose `seq` is newer than the highest
/// seq already present on the SD mirror. No-op if there's nothing
/// to copy or if the SD read probe itself fails (in which case
/// `SD_ONLINE` flips off).
///
/// Uses the SD file itself as the "last mirrored" marker: we scan
/// it, track the max seq we see, and copy flash entries with
/// `seq > sd_max`. No separate state file - if the SD log is
/// trimmed, truncated, or restored from a backup the next backfill
/// self-corrects against whatever's currently on the card.
pub fn backfill_sd(fs: &FlashFs, storage: &mut EspVolumeManager) {
    if !SD_ONLINE.load(Ordering::Relaxed) {
        return;
    }

    // Phase 1: learn the SD's highest seq.
    let mut sd_max: u32 = 0;
    let scan = for_each_line(storage, |line| {
        if let Some(entry) = parse_log_line(line) {
            if entry.seq > sd_max {
                sd_max = entry.seq;
            }
        }
        ControlFlow::Continue(())
    });
    if scan.is_err() {
        log::warn!("event_log: backfill SD scan failed, marking offline");
        SD_ONLINE.store(false, Ordering::Relaxed);
        return;
    }

    // Phase 2: walk the flash log, copying anything past sd_max.
    // On SD write failure we flip SD_ONLINE off and stop early so
    // we don't spam warnings. The entries that did land are
    // committed - next probe resumes from whatever the SD now holds.
    let mut copied = 0u32;
    let mut aborted = false;
    let _ = fs.for_each_line(FLASH_LOG_PATH, |line| {
        if aborted {
            return ControlFlow::Break(());
        }
        let Some(entry) = parse_log_line(line) else {
            return ControlFlow::Continue(());
        };
        if entry.seq <= sd_max {
            return ControlFlow::Continue(());
        }

        // Re-assemble the on-disk line: parser strips the trailing
        // newline, and we want the mirror to be byte-identical.
        let mut buf: heapless::String<96> = heapless::String::new();
        if core::fmt::Write::write_fmt(&mut buf, format_args!("{}\n", line)).is_err() {
            return ControlFlow::Continue(());
        }
        if let Err(e) = append_bytes(storage, buf.as_bytes()) {
            log::warn!(
                "event_log: backfill SD append failed at seq {} ({:?}), marking offline",
                entry.seq, e,
            );
            SD_ONLINE.store(false, Ordering::Relaxed);
            aborted = true;
            return ControlFlow::Break(());
        }
        copied += 1;
        ControlFlow::Continue(())
    });

    if copied > 0 {
        log::info!(
            "event_log: backfilled {} entries to SD (from seq {} -> {})",
            copied, sd_max, sd_max + copied,
        );
    }
}


/// Scan the on-flash event log at boot to recover the monotonic
/// sequence counter. Must be called once from `SystemManager::init`
/// after `FlashFs::mount_or_format` and before any
/// [`try_log`] / [`log_boot`] call.
pub fn init_seq_from_flash(fs: &FlashFs) {
    let mut max_seq = 0u32;
    let _ = fs.for_each_line(FLASH_LOG_PATH, |line| {
        if let Some(entry) = parse_log_line(line) {
            if entry.seq > max_seq {
                max_seq = entry.seq;
            }
        }
        ControlFlow::Continue(())
    });
    NEXT_SEQ.store(max_seq.wrapping_add(1), Ordering::Relaxed);
    log::info!("event_log: resumed at seq {}", max_seq + 1);
}

/// Classify and append `event` to the flash log and, if the SD
/// mirror is online, to the SD log as well. No-op if the event
/// isn't loggable.
pub fn try_log(
    fs: &FlashFs,
    storage: &mut EspVolumeManager,
    time: &TimeData,
    event: &SystemEvent,
) {
    let Some(logged) = classify_for_log(event) else { return };
    write_line(fs, storage, time, logged);
}

/// Record a "boot" line at startup. Separate entry point because
/// there's no `SystemEvent::Boot` - boot is just "we started
/// running", emitted directly from the manager after the RTC + FS
/// are up.
pub fn log_boot(
    fs: &FlashFs,
    storage: &mut EspVolumeManager,
    time: &TimeData,
) {
    write_line(fs, storage, time, LoggedEvent { tag: "boot", detail: None });
}

fn write_line(
    fs: &FlashFs,
    storage: &mut EspVolumeManager,
    time: &TimeData,
    event: LoggedEvent,
) {
    let seq = NEXT_SEQ.fetch_add(1, Ordering::Relaxed);

    // 64 bytes holds "<u32>,YYYY-MM-DDTHH:MM:SS,<tag>,<u32>\n"
    // with plenty of slack (u32 is 10 digits max, tag ≤ 20).
    let mut line: heapless::String<64> = heapless::String::new();
    let fmt_result = match event.detail {
        Some(n) => write!(
            &mut line,
            "{},{:04}-{:02}-{:02}T{:02}:{:02}:{:02},{},{}\n",
            seq,
            time.year, time.month, time.day,
            time.hour, time.minute, time.second,
            event.tag, n,
        ),
        None => write!(
            &mut line,
            "{},{:04}-{:02}-{:02}T{:02}:{:02}:{:02},{}\n",
            seq,
            time.year, time.month, time.day,
            time.hour, time.minute, time.second,
            event.tag,
        ),
    };
    if fmt_result.is_err() {
        log::warn!("event_log: line buffer overflow, dropping seq {}", seq);
        return;
    }

    // Flash side: always write, best-effort.
    if let Err(e) = fs.append_line(FLASH_LOG_PATH, line.as_bytes()) {
        log::warn!("event_log: flash append failed ({:?})", e);
    }

    // SD side: gated on the `SD_ONLINE` flag. A successful boot
    // probe (or the Settings "Initialize SD card" button) flips
    // this on; the first runtime failure flips it off to stop
    // warn-spam if the card got yanked. Flash remains
    // authoritative either way.
    if SD_ONLINE.load(Ordering::Relaxed) {
        if let Err(e) = append_bytes(storage, line.as_bytes()) {
            log::warn!("event_log: sd append failed ({:?}), marking SD offline", e);
            SD_ONLINE.store(false, Ordering::Relaxed);
        }
    }
}

/// Append one line to the log.
fn append_bytes(
    storage: &mut EspVolumeManager,
    bytes: &[u8],
) -> Result<(), SdmmcError<SdCardError>> {
    with_log_file(storage, FileMode::ReadWriteCreateOrAppend, true, |storage, file| {
        let result = storage.write(file, bytes);
        let _ = storage.flush_file(file);
        result
    })
}

/// Open `/system/logs/events.log` in the given mode, run `f` with
/// the open file handle, and guarantee cleanup of all volume / dir
/// / file handles on every path (happy, `f` returning Err, cleanup
/// errors).
///
/// `create_dirs`:
///   * `true`  - create `/system/` and `/system/logs/` if missing
///               (writers). Combined with `ReadWriteCreateOrAppend`
///               this gets "first write on a blank card" working.
///   * `false` - if either directory is missing, return
///               `SdmmcError::NotFound`; the caller maps that to an
///               empty result. Readers use this so an empty card
///               reads cleanly instead of erroring.
fn with_log_file<T, F>(
    storage: &mut EspVolumeManager,
    mode: FileMode,
    create_dirs: bool,
    f: F,
) -> Result<T, SdmmcError<SdCardError>>
where
    F: FnOnce(&mut EspVolumeManager, RawFile) -> Result<T, SdmmcError<SdCardError>>,
{
    let vol = storage.open_raw_volume(VolumeIdx(0))?;
    let root = match storage.open_root_dir(vol) {
        Ok(d) => d,
        Err(e) => {
            let _ = storage.close_volume(vol);
            return Err(e);
        }
    };

    let sysdir = match open_or_create_dir(storage, root, SD_SYSTEM_DIR, create_dirs) {
        Ok(d) => d,
        Err(e) => {
            let _ = storage.close_dir(root);
            let _ = storage.close_volume(vol);
            return Err(e);
        }
    };

    let logsdir = match open_or_create_dir(storage, sysdir, SD_LOGS_DIR, create_dirs) {
        Ok(d) => d,
        Err(e) => {
            let _ = storage.close_dir(sysdir);
            let _ = storage.close_dir(root);
            let _ = storage.close_volume(vol);
            return Err(e);
        }
    };

    let file = match storage.open_file_in_dir(logsdir, SD_LOG_FILENAME, mode) {
        Ok(f) => f,
        Err(e) => {
            let _ = storage.close_dir(logsdir);
            let _ = storage.close_dir(sysdir);
            let _ = storage.close_dir(root);
            let _ = storage.close_volume(vol);
            return Err(e);
        }
    };

    let result = f(storage, file);

    let _ = storage.close_file(file);
    let _ = storage.close_dir(logsdir);
    let _ = storage.close_dir(sysdir);
    let _ = storage.close_dir(root);
    let _ = storage.close_volume(vol);
    result
}

/// Open `name` inside `parent`. If `NotFound` and `create` is true,
/// create it and retry the open. Any other error (or NotFound when
/// `create` is false) is returned as-is for the caller to handle.
fn open_or_create_dir(
    storage: &mut EspVolumeManager,
    parent: RawDirectory,
    name: &str,
    create: bool,
) -> Result<RawDirectory, SdmmcError<SdCardError>> {
    match storage.open_dir(parent, name) {
        Ok(d) => Ok(d),
        Err(SdmmcError::NotFound) if create => {
            storage.make_dir_in_dir(parent, name)?;
            storage.open_dir(parent, name)
        }
        Err(e) => Err(e),
    }
}

// -- Read API ---------------------------------------------------------------

/// Maximum line length accepted by [`for_each_line`]. Anything
/// longer gets truncated - the writer never emits lines larger than
/// about 35 bytes, so a generous 80-byte window covers both current
/// format and a future detail column growth.
const READ_LINE_CAP: usize = 80;

/// Stream every line of `/system/logs/events.log` on the SD card through `callback`.
///
/// The callback receives the raw line (without the trailing newline)
/// as `&str`. It may return `ControlFlow::Break(())` to stop the scan
/// early - useful for readers that bound their own result size.
///
/// Returns the number of lines visited (including any partial trailing
/// line that was skipped due to overflow). If the log file or
/// `/system/` directory doesn't exist yet, returns `Ok(0)` - a fresh
/// card with no boot line written yet is a legitimate state, not an
/// error.
pub fn for_each_line<F>(
    storage: &mut EspVolumeManager,
    mut callback: F,
) -> Result<usize, SdmmcError<SdCardError>>
where
    F: FnMut(&str) -> ControlFlow<()>,
{
    let result = with_log_file(storage, FileMode::ReadOnly, false, |storage, file| {
        let mut io_buf = [0u8; 256];
        let mut line_buf: heapless::Vec<u8, READ_LINE_CAP> = heapless::Vec::new();
        let mut visited = 0usize;
        let mut dropped_line = false; // true after we've truncated the current line

        loop {
            let n = storage.read(file, &mut io_buf)?;
            if n == 0 {
                break;
            }
            for &b in &io_buf[..n] {
                if b == b'\n' {
                    if !dropped_line {
                        if let Ok(s) = core::str::from_utf8(&line_buf) {
                            // Strip one trailing \r for CRLF tolerance.
                            let s = s.strip_suffix('\r').unwrap_or(s);
                            if callback(s).is_break() {
                                return Ok(visited + 1);
                            }
                        }
                    }
                    visited += 1;
                    line_buf.clear();
                    dropped_line = false;
                } else if !dropped_line {
                    if line_buf.push(b).is_err() {
                        // Line exceeded READ_LINE_CAP - drop the rest
                        // until the next newline.
                        dropped_line = true;
                        line_buf.clear();
                    }
                }
            }
        }

        // Trailing line without newline (e.g. interrupted write).
        if !line_buf.is_empty() && !dropped_line {
            if let Ok(s) = core::str::from_utf8(&line_buf) {
                let _ = callback(s);
                visited += 1;
            }
        }
        Ok(visited)
    });

    // A missing /system/ dir or file just means "no log yet".
    match result {
        Ok(n) => Ok(n),
        Err(SdmmcError::NotFound) => Ok(0),
        Err(e) => Err(e),
    }
}

/// Read a page of raw log lines starting at `start_line` (0-indexed).
/// Fills `out` with up to `out.len()` parsed entries and returns how
/// many were written. Lines that fail to parse are skipped silently
/// (they still advance `start_line`'s count so pagination stays
/// stable line-number-wise).
///
/// Intended for a future text-viewer screen: call with
/// `start_line = page * page_size` and a fixed-size `out` buffer.
pub fn read_page(
    storage: &mut EspVolumeManager,
    start_line: usize,
    out: &mut [LogEntry],
) -> Result<usize, SdmmcError<SdCardError>> {
    let mut skipped = 0usize;
    let mut written = 0usize;
    for_each_line(storage, |line| {
        if skipped < start_line {
            skipped += 1;
            return ControlFlow::Continue(());
        }
        if written >= out.len() {
            return ControlFlow::Break(());
        }
        if let Some(entry) = parse_log_line(line) {
            out[written] = entry;
            written += 1;
        }
        ControlFlow::Continue(())
    })?;
    Ok(written)
}

/// Ring-buffer the last `out.len()` entries whose `tag` matches
/// `tag` into `out`, oldest-first. Returns the number of matches
/// written (which is `min(out.len(), total_matches)`).
///
/// The scan is forward-only (one pass from start to end) - fine while
/// the log is under a few hundred KB. When the file grows large
/// enough that a full scan stalls the UI, add rotation or a reverse
/// reader.
pub fn read_recent_by_tag(
    storage: &mut EspVolumeManager,
    tag: &str,
    out: &mut [LogEntry],
) -> Result<usize, SdmmcError<SdCardError>> {
    if out.is_empty() {
        return Ok(0);
    }
    // Ring layout: `head` is the write index. After the first full
    // pass, `filled == out.len()` and new writes overwrite the
    // oldest slot.
    let mut head = 0usize;
    let mut filled = 0usize;

    for_each_line(storage, |line| {
        if let Some(entry) = parse_log_line(line) {
            if entry.tag.as_str() == tag {
                out[head] = entry;
                head = (head + 1) % out.len();
                if filled < out.len() {
                    filled += 1;
                }
            }
        }
        ControlFlow::Continue(())
    })?;

    if filled < out.len() {
        // No wraparound - entries sit contiguously at [0, filled).
        return Ok(filled);
    }

    // Ring wrapped. Rotate so the oldest sits at index 0. With
    // `head` pointing at the oldest slot (= the next write target),
    // we want `out.rotate_left(head)`.
    out.rotate_left(head);
    Ok(filled)
}

