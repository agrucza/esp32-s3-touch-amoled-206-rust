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

use crate::system::flash_fs::FlashFs;
use crate::system::sd_fs::SdFs;
use app_core::data::TimeData;
use app_core::events::{LoggedEvent, SystemEvent, classify_for_log};
use app_core::log::parse_log_line;
use core::fmt::Write as _;
use core::ops::ControlFlow;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Log file path - identical on both backends so the SD mirror is
/// a byte-for-byte copy of the flash log.
const LOG_PATH: &str = "/system/logs/events.log";

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

/// Scan the on-flash event log at boot to recover the monotonic
/// sequence counter. Must be called once from `SystemManager::init`
/// after `FlashFs::mount_or_format` and before any
/// [`try_log`] / [`log_boot`] call.
pub fn init_seq_from_flash(fs: &mut FlashFs) {
    let mut max_seq = 0u32;
    let _ = fs.for_each_line(LOG_PATH, |line| {
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

/// Copy any flash log entry whose `seq` is newer than the highest
/// seq already present on the SD mirror. No-op if the mirror is
/// offline or the read scan fails (in which case `SD_ONLINE` flips
/// off).
///
/// Uses the SD file itself as the "last mirrored" marker: scan it,
/// track the max seq we see, copy flash entries with `seq > sd_max`.
/// No separate state file - if the SD log is trimmed, truncated, or
/// restored from a backup, the next back-fill self-corrects against
/// whatever is currently on the card.
pub fn backfill_sd(fs: &mut FlashFs, sd: &mut SdFs) {
    if !SD_ONLINE.load(Ordering::Relaxed) {
        return;
    }

    // Phase 1: learn the SD's highest seq.
    let mut sd_max: u32 = 0;
    let scan = sd.for_each_line(LOG_PATH, |line| {
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
    // we don't spam warnings. Entries that already landed are
    // committed - next probe resumes from whatever SD now holds.
    let mut copied = 0u32;
    let mut aborted = false;
    let _ = fs.for_each_line(LOG_PATH, |line| {
        if aborted { return ControlFlow::Break(()); }
        let Some(entry) = parse_log_line(line) else {
            return ControlFlow::Continue(());
        };
        if entry.seq <= sd_max {
            return ControlFlow::Continue(());
        }

        // Re-assemble the on-disk line: parser strips the trailing
        // newline, we want the mirror to be byte-identical.
        let mut buf: heapless::String<96> = heapless::String::new();
        if core::fmt::Write::write_fmt(&mut buf, format_args!("{}\n", line)).is_err() {
            return ControlFlow::Continue(());
        }
        if let Err(e) = sd.append_line(LOG_PATH, buf.as_bytes()) {
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

/// Classify and append `event` to the flash log and, if the SD
/// mirror is online, to the SD log as well. No-op if the event
/// isn't loggable.
pub fn try_log(
    fs: &mut FlashFs,
    sd: &mut SdFs,
    time: &TimeData,
    event: &SystemEvent,
) {
    let Some(logged) = classify_for_log(event) else { return };
    write_line(fs, sd, time, logged);
}

/// Record a "boot" line at startup. Separate entry point because
/// there's no `SystemEvent::Boot` - boot is just "we started
/// running", emitted directly from the manager after the RTC + FS
/// are up.
pub fn log_boot(fs: &mut FlashFs, sd: &mut SdFs, time: &TimeData) {
    write_line(fs, sd, time, LoggedEvent { tag: "boot", detail: None });
}

fn write_line(
    fs: &mut FlashFs,
    sd: &mut SdFs,
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
    if let Err(e) = fs.append_line(LOG_PATH, line.as_bytes()) {
        log::warn!("event_log: flash append failed ({:?})", e);
    }

    // SD side: gated on the `SD_ONLINE` flag. First runtime failure
    // flips it off to stop warn-spam if the card got yanked. Flash
    // remains authoritative either way.
    if SD_ONLINE.load(Ordering::Relaxed) {
        if let Err(e) = sd.append_line(LOG_PATH, line.as_bytes()) {
            log::warn!("event_log: sd append failed ({:?}), marking SD offline", e);
            SD_ONLINE.store(false, Ordering::Relaxed);
        }
    }
}

