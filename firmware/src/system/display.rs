use crate::config::DisplayConfig;
use crate::display_hal::{self, CO5300, EspQspi};
use embassy_time::{Duration, Timer};
use esp_hal::{
    dma::DmaChannelFor,
    gpio::{Output, interconnect::PeripheralOutput},
    spi::master::AnySpi,
};

/// Concrete type of the display handle the rest of the firmware uses.
/// Parameterized over the esp-hal peripheral lifetime `'d`; the
/// framebuffer is `'static` because it's leaked out of a PSRAM Vec
/// at init time in the manager.
pub type Display<'d> = CO5300<'static, EspQspi<'d>, Output<'d>>;

/// Display power-management state. Transitions are driven by idle
/// time since the last user-input event (touch / swipe / button).
///
/// * `Active`: normal running state at full brightness.
/// * `Dim`: brightness register dropped, rendering continues normally.
///   This is the first-stage power save and is the cheapest to enter
///   and leave (single DCS command over SPI).
/// * `Off`: `DISPOFF` issued, the entire render path is skipped until
///   a user event wakes the display again. Deepest power save short
///   of a full light-sleep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayState {
    Active,
    Dim,
    Off,
}

/// Perform the full display hardware init sequence.
///
/// Builds the QSPI bus, performs the reset pulse, sends init commands,
/// sleep out, and display on. Returns the ready-to-use display handle.
pub async fn init_display<'d, 'fb>(
    spi: impl esp_hal::spi::master::Instance + 'd,
    sclk: impl PeripheralOutput<'d>,
    sio0: impl PeripheralOutput<'d>,
    sio1: impl PeripheralOutput<'d>,
    sio2: impl PeripheralOutput<'d>,
    sio3: impl PeripheralOutput<'d>,
    cs: impl PeripheralOutput<'d>,
    dma: impl DmaChannelFor<AnySpi<'d>>,
    reset_pin: Output<'d>,
    fb: &'fb mut [u8],
) -> CO5300<'fb, EspQspi<'d>, Output<'d>> {
    let bus = display_hal::build_spi(spi, sclk, sio0, sio1, sio2, sio3, cs, dma);
    let mut display = CO5300::new(bus, reset_pin, fb);

    // Hardware reset: short low pulse then settle
    display.reset_high();
    Timer::after(Duration::from_millis(10)).await;
    display.reset_low();
    Timer::after(Duration::from_millis(10)).await;
    display.reset_high();
    Timer::after(Duration::from_millis(120)).await;

    log::info!("Display: initializing CO5300...");
    display.init().await;
    display.wake().await;
    Timer::after(Duration::from_millis(120)).await;
    display.display_on().await;
    Timer::after(Duration::from_millis(70)).await;
    log::info!("Display: ready");

    display
}

/// Apply a display-state transition. Issues the necessary DCS
/// commands over SPI: brightness changes for Active/Dim, `DISPOFF`
/// for Off, and `DISPON` + brightness when waking from Off.
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
        }
        DisplayState::Active => {
            if waking_from_off {
                display.display_on().await;
            }
            display.set_brightness(config.brightness_active).await;
        }
        DisplayState::Dim => {
            if waking_from_off {
                display.display_on().await;
            }
            display.set_brightness(config.brightness_dim).await;
        }
    }
    log::info!("display: {:?} -> {:?}", from, to);
    waking_from_off
}
