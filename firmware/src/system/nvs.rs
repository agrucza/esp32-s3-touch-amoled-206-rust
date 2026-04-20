//! Flash-backed key/value persistence ("NVS" on ESP-IDF, here
//! built on `esp-storage` + `sequential-storage`).
//!
//! See the module-level commentary below for the flash layout.
//! The public surface is the [`Nvs`] handle with its
//! `load_config` / `save_config` / `load_alarms` / `save_alarms`
//! methods. The manager owns one `Nvs` instance and calls it
//! directly in `init()` (boot-time load) and from the effect
//! executor (save on demand).
//!
//! ## Why not ESP-IDF NVS?
//!
//! The standard ESP-IDF `nvs_flash` API requires the IDF
//! component framework (`esp-idf-hal`, `esp-idf-sys`), which
//! pulls in a C toolchain and a completely different runtime
//! than our bare-metal `esp-hal` build. `esp-storage` +
//! `sequential-storage` give us the same capability (append-only
//! KV store, wear-leveling, crash-safe) in pure Rust with no
//! extra build tooling.
//!
//! ## Flash layout
//!
//! The ESP32-S3 on this board has 32 MB of flash. The bootloader
//! partition table (from `esp-bootloader-esp-idf`) defines:
//!
//! ```text
//!   0x009000 +  24 KB  nvs       (ESP-IDF's own NVS; we leave
//!                                 this alone in case `esp-wifi`
//!                                 eventually wants it)
//!   0x00f000 +   4 KB  phy_init
//!   0x010000 + 16 MB   factory   (firmware image)
//!   0xfb0000 - 0x2000000       (~17 MB unallocated tail)
//! ```
//!
//! We carve a dedicated 64 KB region (16 flash sectors) out of
//! the unallocated tail for our own config store. That's more
//! than `sequential-storage` minimally needs (~2 sectors) and
//! leaves headroom for future keys while staying well clear of
//! the firmware image.
//!
//! ## Versioning
//!
//! Each stored record is wrapped in a `Stored<T> { version, inner
//! }`. On load we compare `version` to the current build's
//! constant; if they differ (firmware upgrade changed the layout)
//! we drop the record and fall back to `T::default()`. This
//! prevents a stale postcard-deserialised record from ever
//! surfacing with wrong field values. Bump the per-type version
//! constant whenever the persisted shape changes.

use app_core::config::Config;
use app_core::data::NvsUsage;
use app_core::ui::types::AlarmState;
use embassy_embedded_hal::adapter::BlockingAsync;
use esp_hal::peripherals::FLASH;
use esp_storage::FlashStorage;
use sequential_storage::{
    cache::NoCache,
    map::{MapConfig, MapStorage, PostcardValue},
};
use serde::{Deserialize, Serialize};

/// Start offset of the config-store region, in bytes from the
/// start of flash. Sector-aligned (4 KB).
pub const FLASH_REGION_START: u32 = 0x00FB_0000;

/// Size of the config-store region in bytes. 64 KB = 16 sectors
/// = comfortable headroom for the store's rotating ring without
/// touching anything adjacent.
pub const FLASH_REGION_SIZE: u32 = 64 * 1024;

/// End offset (exclusive) of the config-store region.
pub const FLASH_REGION_END: u32 = FLASH_REGION_START + FLASH_REGION_SIZE;

// -- Keys (stable wire identifiers; never reuse numbers) --------------------

/// `app_core::config::Config` snapshot.
const KEY_CONFIG: u32 = 0x0001;
/// `app_core::ui::types::AlarmState` snapshot.
const KEY_ALARMS: u32 = 0x0002;

// -- Version tags (bump when layout of the wrapped type changes) -------------

const VERSION_CONFIG: u8 = 1;
const VERSION_ALARMS: u8 = 1;

// -- Stored wrapper types ----------------------------------------------------

#[derive(Serialize, Deserialize)]
struct StoredConfig {
    version: u8,
    inner: Config,
}
impl PostcardValue<'_> for StoredConfig {}

#[derive(Serialize, Deserialize)]
struct StoredAlarms {
    version: u8,
    inner: AlarmState,
}
impl PostcardValue<'_> for StoredAlarms {}

// -- Nvs handle --------------------------------------------------------------

type Storage<'d> = MapStorage<u32, BlockingAsync<FlashStorage<'d>>, NoCache>;

/// Flash-backed persistence handle. Held by `SystemManager`; one
/// instance per boot. All methods are async because
/// `sequential-storage` only ships an async API (the underlying
/// flash ops are synchronous; the `BlockingAsync` wrapper just
/// bridges the traits).
pub struct Nvs<'d> {
    storage: Storage<'d>,
    /// Scratch buffer for postcard (de)serialisation. Must be
    /// large enough to hold the longest wrapped record. The
    /// largest record today is `StoredAlarms` ~= 1 byte version
    /// + 8 alarms * ~4 bytes each + state flags = well under
    /// 256 bytes. 512 is generous.
    buf: [u8; 512],
}

impl<'d> Nvs<'d> {
    /// Create a new persistence handle. Takes the raw `FLASH`
    /// peripheral singleton; initialises the underlying
    /// `FlashStorage` + `MapStorage`.
    pub fn new(flash: FLASH<'d>) -> Self {
        let flash_storage = BlockingAsync::new(FlashStorage::new(flash));
        let config = MapConfig::new(FLASH_REGION_START..FLASH_REGION_END);
        Self {
            storage: MapStorage::new(flash_storage, config, NoCache::new()),
            buf: [0u8; 512],
        }
    }

    /// Load the stored `Config`, or `None` if no record is
    /// present, the stored version doesn't match this build, or
    /// deserialisation fails. Callers fall back to
    /// `Config::default()` in those cases.
    pub async fn load_config(&mut self) -> Option<Config> {
        match self.storage.fetch_item::<StoredConfig>(&mut self.buf, &KEY_CONFIG).await {
            Ok(Some(stored)) if stored.version == VERSION_CONFIG => Some(stored.inner),
            Ok(Some(stored)) => {
                log::warn!(
                    "nvs: stored Config version {} != build {}; ignoring",
                    stored.version, VERSION_CONFIG,
                );
                None
            }
            Ok(None) => None,
            Err(e) => {
                log::warn!("nvs: load_config failed: {:?}", e);
                None
            }
        }
    }

    /// Persist the current `Config`. Append-only; the underlying
    /// store rotates sectors so no explicit erase is needed.
    pub async fn save_config(&mut self, config: &Config) {
        let stored = StoredConfig { version: VERSION_CONFIG, inner: *config };
        if let Err(e) = self.storage.store_item(&mut self.buf, &KEY_CONFIG, &stored).await {
            log::warn!("nvs: save_config failed: {:?}", e);
        }
    }

    /// Load the stored `AlarmState`, or `None` if no record /
    /// version mismatch / deserialisation error.
    pub async fn load_alarms(&mut self) -> Option<AlarmState> {
        match self.storage.fetch_item::<StoredAlarms>(&mut self.buf, &KEY_ALARMS).await {
            Ok(Some(stored)) if stored.version == VERSION_ALARMS => Some(stored.inner),
            Ok(Some(stored)) => {
                log::warn!(
                    "nvs: stored AlarmState version {} != build {}; ignoring",
                    stored.version, VERSION_ALARMS,
                );
                None
            }
            Ok(None) => None,
            Err(e) => {
                log::warn!("nvs: load_alarms failed: {:?}", e);
                None
            }
        }
    }

    /// Persist the alarm list. Transient runtime flags
    /// (`active_hw` / `alerting` / `snoozed`) are skipped during
    /// serialisation via `#[serde(skip)]` on the struct.
    pub async fn save_alarms(&mut self, alarms: &AlarmState) {
        let stored = StoredAlarms { version: VERSION_ALARMS, inner: *alarms };
        if let Err(e) = self.storage.store_item(&mut self.buf, &KEY_ALARMS, &stored).await {
            log::warn!("nvs: save_alarms failed: {:?}", e);
        }
    }

    /// Summarise the config store: count of *unique keys* that
    /// have a value, plus the configured region size. Used by
    /// the settings screen via `SystemEvent::NvsUsageUpdated`.
    ///
    /// `fetch_all_items` walks every appended record including
    /// stale (superseded) versions of the same key - sequential-
    /// storage only discards them during sector rotation. We
    /// deduplicate here so the reported count matches user
    /// intuition ("how many different things are stored?")
    /// rather than reflecting internal wear.
    pub async fn usage_summary(&mut self) -> NvsUsage {
        // Upper bound on distinct keys we expect. 16 is generous
        // (we have 2 today: Config and Alarms).
        const MAX_UNIQUE_KEYS: usize = 16;
        let mut seen: heapless::Vec<u32, MAX_UNIQUE_KEYS> = heapless::Vec::new();

        match self.storage.fetch_all_items(&mut self.buf).await {
            Ok(mut iter) => {
                let mut scratch = [0u8; 512];
                loop {
                    match iter.next::<&[u8]>(&mut scratch).await {
                        Ok(Some((key, _value))) => {
                            if !seen.contains(&key) {
                                // Silently drop if we somehow
                                // exceed MAX_UNIQUE_KEYS - the
                                // count is a UI hint, not a
                                // correctness requirement.
                                let _ = seen.push(key);
                            }
                        }
                        Ok(None) => break,
                        Err(e) => {
                            log::warn!("nvs: usage_summary iter failed: {:?}", e);
                            break;
                        }
                    }
                }
            }
            Err(e) => {
                log::warn!("nvs: usage_summary open failed: {:?}", e);
            }
        }
        NvsUsage {
            records: seen.len() as u32,
            total_bytes: FLASH_REGION_SIZE,
        }
    }

    /// Wipe the entire 64 KB config-store region. Irrecoverable -
    /// every stored record (Config, AlarmState, anything else)
    /// is gone. Intended for the "reset to defaults" path in the
    /// settings screen, or manual recovery from a corrupt store.
    ///
    /// On the next boot the firmware will find no records and
    /// fall back to `Config::default()` / `AlarmState::default()`
    /// for everything.
    pub async fn erase_all(&mut self) {
        if let Err(e) = self.storage.erase_all().await {
            log::warn!("nvs: erase_all failed: {:?}", e);
        }
    }
}
