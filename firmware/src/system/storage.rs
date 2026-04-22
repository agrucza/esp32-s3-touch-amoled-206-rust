use crate::sdcard_hal::{self, EspVolumeManager};
use drivers::sdcard::{DirEntry, RtcTimeSource, SdCardError, SdmmcError, VolumeIdx};
use esp_hal::gpio::{Output, interconnect::{PeripheralInput, PeripheralOutput}};

/// Directories under `/system/` on the SD card that [`reset_sd`]
/// wipes as part of a factory reset. Deliberately narrower than
/// the flash-side reset list - the SD is user-visible removable
/// media, so we only touch directories we authored. User content
/// elsewhere on the card is preserved.
///
/// Directory names are relative to `/system/` on the card.
const SD_RESET_CHILDREN: &[&str] = &["logs"];
const SD_SYSTEM_DIR:     &str    = "system";

/// Construct the SD `VolumeManager` unconditionally. No I/O at this
/// stage - the underlying `SdCard` defers its ACMD41 identification
/// until the first real read, so this succeeds even if no card is
/// in the slot. Use [`probe_sd`] afterwards to detect card presence.
///
/// Returning `VolumeManager` (not `Option`) keeps the SPI peripheral
/// and chip-select pin alive for the whole boot; the runtime can
/// then re-probe on demand (Stage C "Initialize SD" button) without
/// re-plumbing any hardware tokens.
pub fn init_sd<'d>(
    spi: impl esp_hal::spi::master::Instance + 'd,
    sck: impl PeripheralOutput<'d>,
    mosi: impl PeripheralOutput<'d>,
    miso: impl PeripheralInput<'d>,
    cs: Output<'d>,
) -> EspVolumeManager<'d> {
    log::info!("SD card: building volume manager (card access deferred)");
    let sd_card = sdcard_hal::build_sdcard(spi, sck, mosi, miso, cs);
    EspVolumeManager::new(sd_card, RtcTimeSource)
}

/// Try to open + immediately close volume 0. Returns `true` if a
/// card is present and the FAT MBR is readable. Safe to call
/// repeatedly - no side effects beyond one SD read cycle.
///
/// This is the "is there a card?" check. Call at boot after
/// [`init_sd`] and again whenever the user taps the Storage
/// "Initialize SD card" button.
pub fn probe_sd(storage: &mut EspVolumeManager) -> bool {
    match storage.open_raw_volume(VolumeIdx(0)) {
        Ok(vol) => {
            let _ = storage.close_volume(vol);
            log::info!("SD card: present (volume 0 readable)");
            true
        }
        Err(e) => {
            log::info!("SD card: absent or unreadable ({:?})", e);
            false
        }
    }
}

/// Factory-reset counterpart on the SD side. Delete every file
/// inside each `/system/<child>/` directory listed in
/// [`SD_RESET_CHILDREN`]. The directory structure itself stays in
/// place, ready for the next mirror write.
///
/// Best-effort: errors are logged and the walk continues. Caller
/// is responsible for gating on mirror-online state.
pub fn reset_sd(storage: &mut EspVolumeManager) {
    match walk_reset(storage) {
        Ok(n) => log::info!("SD card: reset complete, {} files removed", n),
        Err(e) => log::warn!("SD card: reset errored ({:?})", e),
    }
}

fn walk_reset(storage: &mut EspVolumeManager) -> Result<u32, SdmmcError<SdCardError>> {
    let vol = storage.open_raw_volume(VolumeIdx(0))?;
    let root = match storage.open_root_dir(vol) {
        Ok(d) => d,
        Err(e) => {
            let _ = storage.close_volume(vol);
            return Err(e);
        }
    };
    let sysdir = match storage.open_dir(root, SD_SYSTEM_DIR) {
        Ok(d) => d,
        Err(SdmmcError::NotFound) => {
            // Card has no /system/ dir - nothing to wipe.
            let _ = storage.close_dir(root);
            let _ = storage.close_volume(vol);
            return Ok(0);
        }
        Err(e) => {
            let _ = storage.close_dir(root);
            let _ = storage.close_volume(vol);
            return Err(e);
        }
    };

    let mut removed = 0u32;
    for child in SD_RESET_CHILDREN {
        let child_dir = match storage.open_dir(sysdir, *child) {
            Ok(d) => d,
            Err(SdmmcError::NotFound) => continue,
            Err(e) => {
                log::warn!("SD reset: open /system/{} failed: {:?}", child, e);
                continue;
            }
        };

        // Two phase: collect names (can't delete during
        // iterate_dir), then delete. Cap at 32 entries per
        // subdirectory; covers current logs file plus ample
        // rotation / future files.
        let mut to_delete: heapless::Vec<DirEntry, 32> = heapless::Vec::new();
        let _ = storage.iterate_dir(child_dir, |entry: &DirEntry| {
            if entry.attributes.is_directory() {
                return;
            }
            let _ = to_delete.push(entry.clone());
        });

        for entry in &to_delete {
            match storage.delete_file_in_dir(child_dir, entry.name.clone()) {
                Ok(()) => removed += 1,
                Err(e) => log::warn!(
                    "SD reset: delete /system/{}/{:?} failed: {:?}",
                    child, entry.name, e,
                ),
            }
        }

        let _ = storage.close_dir(child_dir);
    }

    let _ = storage.close_dir(sysdir);
    let _ = storage.close_dir(root);
    let _ = storage.close_volume(vol);
    Ok(removed)
}
