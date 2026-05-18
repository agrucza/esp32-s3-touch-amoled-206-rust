use app_core::config::DisplayConfig;
use embassy_time::{Duration, Timer};
use esp_hal::gpio::Output;
use firmware_hal::display::{CO5300, EspQspi};

/// Concrete type of the display handle the rest of the firmware uses.
/// Parameterized over the esp-hal peripheral lifetime `'d`; the
/// framebuffer is `'static` because it's leaked out of a Vec at init
/// time in the manager.
pub type Display<'d> = CO5300<'static, EspQspi<'d>, Output<'d>>;

// `DisplayState` lives in `app_core::ui::types` so Model / UI can
// reason about display power state without touching hardware.
// Re-exported here so existing firmware imports keep working.
pub use app_core::ui::types::DisplayState;

// The display init sequence (QSPI bus build, reset pulse, CO5300 init,
// wake, display-on) lives in `firmware-hal` so every board shares one
// implementation. Re-export it here so the existing
// `display::init_display(...)` call site in the manager keeps
// compiling unchanged.
pub use firmware_hal::display::init_display;

/// Apply a display-state transition. Issues the necessary DCS
/// commands over SPI: brightness changes for Active/Dim, the full
/// `DISPOFF` + `SLPIN` sequence for Off, and `SLPOUT` + `DISPON` +
/// brightness when waking from Off.
///
/// Going to `Off` sends both DISPOFF (stops panel output) and
/// SLPIN (shuts down the panel oscillator + booster) because
/// DISPOFF alone still leaves the panel internal logic running at
/// ~mA. SLPIN drops to panel standby (~uA).
///
/// Waking from Off must respect the 120 ms SLPOUT settle window
/// mandated by the CO5300 datasheet before issuing DISPON.
///
/// Returns `true` if the caller should treat this as "waking from
/// Off" and force a full redraw: the caller is expected to reset
/// its dirty-row tracking and set a redraw flag so the next tick
/// pushes the full framebuffer back to the panel's GRAM. Returns
/// `false` for the cheaper Active/Dim transitions where the panel
/// contents are already correct.
pub async fn transition(
    display: &mut Display<'_>,
    from: DisplayState,
    to: DisplayState,
    config: &DisplayConfig,
) -> bool {
    let waking_from_off = from == DisplayState::Off && to != DisplayState::Off;
    match to {
        DisplayState::Off => {
            display.display_off().await;
            // Small settle between DISPOFF and SLPIN so the panel has
            // finished stopping its output scan before we drop the
            // oscillator. Datasheet requires >= 5 ms after SLPIN before
            // the next command; 10 ms is comfortable either way.
            Timer::after(Duration::from_millis(10)).await;
            display.sleep().await;
            Timer::after(Duration::from_millis(10)).await;
        }
        DisplayState::Active => {
            if waking_from_off {
                display.wake().await;
                // Datasheet 7.5.12: wait >= 120 ms after SLPOUT before
                // DISPON. Skipping this can leave the panel booster
                // un-stabilized and produce a first-frame flash.
                Timer::after(Duration::from_millis(120)).await;
                display.display_on().await;
                Timer::after(Duration::from_millis(70)).await;
            }
            display.set_brightness(config.brightness_active).await;
        }
        DisplayState::Dim => {
            if waking_from_off {
                display.wake().await;
                Timer::after(Duration::from_millis(120)).await;
                display.display_on().await;
                Timer::after(Duration::from_millis(70)).await;
            }
            display.set_brightness(config.brightness_dim).await;
        }
    }
    log::info!("display: {:?} -> {:?}", from, to);
    waking_from_off
}
