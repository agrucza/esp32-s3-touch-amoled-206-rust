//! Runtime configuration.
//!
//! Held as a mutable struct on `SystemManager` so call sites always
//! read through `self.config.*`. Today the values come from
//! `Config::default()` (compile-time defaults). In the future we'll
//! add a persistent backing store (NVS via `esp-storage`) and change
//! the init path to `Config::load_or_default()` without touching any
//! of the call sites.
//!
//! This deliberate indirection is the cheapest future-proofing we can
//! do right now: structuring the values as mutable state instead of
//! `const`s means a settings screen or serial-command debug knob can
//! mutate them at runtime and the new values take effect immediately.

/// Display power-management parameters.
#[derive(Debug, Clone, Copy)]
pub struct DisplayConfig {
    /// Seconds of no user activity before the display dims.
    pub dim_timeout_s: u64,
    /// Seconds of no user activity before the display blanks entirely
    /// via `DISPOFF`. Must be greater than `dim_timeout_s`.
    pub off_timeout_s: u64,
    /// Brightness level (0..=255) when the display is fully active.
    /// The panel's init sequence also hardcodes this value, so if the
    /// default here changes, boot will flash the old brightness for a
    /// moment until the first tick reconciles it.
    pub brightness_active: u8,
    /// Brightness level (0..=255) when the display is dimmed. AMOLED
    /// current scales roughly with lit pixels * brightness, so a low
    /// value here is where most of the idle-current savings come from
    /// on a wrist-worn device.
    pub brightness_dim: u8,
}

/// Top-level runtime config. Sub-structs group related settings.
#[derive(Debug, Clone, Copy)]
pub struct Config {
    pub display: DisplayConfig,
}

impl Config {
    /// Compile-time defaults. Tuned for a wrist-worn smartwatch on a
    /// small battery: short dim/off timeouts, dim well below active.
    pub const fn default() -> Self {
        Self {
            display: DisplayConfig {
                dim_timeout_s: 20,
                off_timeout_s: 30,
                brightness_active: 80,
                brightness_dim: 16,
            },
        }
    }
}
