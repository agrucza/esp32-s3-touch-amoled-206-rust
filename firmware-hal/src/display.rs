//! Display init / HAL glue for the CO5300 QSPI AMOLED panel.
//!
//! Used by both the S3 and C6 firmware crates. The function `init_display`
//! takes esp-hal peripheral singletons and a framebuffer, builds the QSPI
//! bus over DMA, runs the CO5300 reset + init sequence, and returns a
//! ready-to-use display handle. The framebuffer can be full-panel
//! ([`FB_BYTES`]) or partial (see [`fb_bytes_for_rows`]); the CO5300 driver
//! clips drawing operations to the actual FB row count.
//!
//! ## DMA / source memory
//!
//! `EspQspi` wraps `SpiDmaBus<Blocking>`. `SpiDmaBus::half_duplex_write`
//! copies the caller's buffer into its own internal DMA TX buffer (internal
//! SRAM) before starting the transfer, so the source framebuffer can live
//! in PSRAM (S3) or internal SRAM (C6) without any extra bounce logic here.

use drivers::display::QspiWrite;
use embassy_time::{Duration, Timer};
use esp_hal::{
    Blocking,
    dma::{DmaChannelFor, DmaRxBuf, DmaTxBuf},
    gpio::{Output, interconnect::{PeripheralInput, PeripheralOutput}},
    spi::master::{Address, AnySpi, Command, Config, DataMode, Spi, SpiDmaBus},
    spi::Mode,
    time::Rate,
};

pub use drivers::display::{
    co5300::{FB_BYTES, HEIGHT, WIDTH, fb_bytes_for_rows},
    CO5300,
};

/// DCS commands used inside `write_pixels`.
const RAMWR:  u8 = 0x2C;
const RAMWRC: u8 = 0x3C;

/// SPI instruction opcodes.
const OPCODE_CTRL:  u8 = 0x02; // 1-wire config/command writes
const OPCODE_PIXEL: u8 = 0x32; // Quad 4-wire pixel writes

/// DMA TX buffer capacity in bytes.
/// 32736 = 8 * 4092 = MAX_DMA_SIZE in esp-hal. Larger buffer = fewer SPI
/// transactions per frame. RX buffer is minimal (4 bytes) since we never
/// read pixel data back.
const DMA_BUF_SIZE: usize = 32736;

/// Wraps the esp-hal DMA SPI bus and provides the [`QspiWrite`] impl that
/// the CO5300 driver calls.
pub struct EspQspi<'d> {
    spi: SpiDmaBus<'d, Blocking>,
}

impl<'d> QspiWrite for EspQspi<'d> {
    type Error = esp_hal::spi::Error;

    /// Send one MIPI DCS command over 1-wire SPI (opcode 0x02).
    async fn write_cmd(&mut self, cmd: u8, params: &[u8]) -> Result<(), Self::Error> {
        self.spi.half_duplex_write(
            DataMode::Single,
            Command::_8Bit(OPCODE_CTRL as u16, DataMode::Single),
            Address::_24Bit((cmd as u32) << 8, DataMode::Single),
            0,
            params,
        )
    }

    /// Write pixel bytes to the display.
    async fn write_pixels(&mut self, first: bool, data: &[u8]) -> Result<(), Self::Error> {
        let mut offset   = 0;
        let mut is_first = first;

        while offset < data.len() {
            let n   = (data.len() - offset).min(DMA_BUF_SIZE);
            let dcs = if is_first { RAMWR } else { RAMWRC };

            self.spi.half_duplex_write(
                DataMode::Quad,
                Command::_8Bit(OPCODE_PIXEL as u16, DataMode::Single),
                Address::_24Bit((dcs as u32) << 8, DataMode::Single),
                0,
                &data[offset..offset + n],
            )?;

            is_first = false;
            offset  += n;
        }
        Ok(())
    }
}

/// Build and configure the QSPI DMA SPI bus for the CO5300.
///
/// The pins are chip-board-specific (see each `firmware-*/src/board.rs`).
/// This function is fully generic over the peripheral types, so the same
/// implementation serves both S3 and C6.
pub fn build_spi<'d>(
    spi:  impl esp_hal::spi::master::Instance + 'd,
    sck:  impl PeripheralOutput<'d>,
    // QSPI data lines are bidirectional in esp-hal 1.1's SPI API -
    // PeripheralInput bound has to be propagated through the wrapper.
    sio0: impl PeripheralInput<'d> + PeripheralOutput<'d>,
    sio1: impl PeripheralInput<'d> + PeripheralOutput<'d>,
    sio2: impl PeripheralInput<'d> + PeripheralOutput<'d>,
    sio3: impl PeripheralInput<'d> + PeripheralOutput<'d>,
    cs:   impl PeripheralOutput<'d>,
    dma:  impl DmaChannelFor<AnySpi<'d>>,
) -> EspQspi<'d> {
    // RX=4 bytes (unused, minimum valid), TX=DMA_BUF_SIZE bytes.
    let (rx_buffer, rx_descriptors, tx_buffer, tx_descriptors) =
        esp_hal::dma_buffers!(4, DMA_BUF_SIZE);
    let dma_rx_buf = DmaRxBuf::new(rx_descriptors, rx_buffer).unwrap();
    let dma_tx_buf = DmaTxBuf::new(tx_descriptors, tx_buffer).unwrap();

    // 66 MHz: at the CO5300 15 ns write spec limit. If this turns out to be
    // unstable in practice we can drop to 40 MHz or push back up to 80 MHz
    // (out of spec but known to work on most panels).
    let spi = Spi::new(
        spi,
        Config::default()
            .with_frequency(Rate::from_mhz(66))
            .with_mode(Mode::_0),
    )
    .unwrap()
    .with_sck(sck)
    .with_sio0(sio0)
    .with_sio1(sio1)
    .with_sio2(sio2)
    .with_sio3(sio3)
    .with_cs(cs)
    .with_dma(dma)
    .with_buffers(dma_rx_buf, dma_tx_buf);

    EspQspi { spi }
}

/// Full display hardware init.
///
/// Builds the QSPI bus over DMA, performs the CO5300 reset pulse, sends the
/// init command sequence, exits sleep, turns the panel on. Returns the
/// ready-to-use display handle.
///
/// `fb` may be a full-panel buffer ([`FB_BYTES`]) or any partial buffer
/// sized via [`fb_bytes_for_rows`]; the CO5300 driver clips drawing
/// operations to the actual FB row count.
pub async fn init_display<'d, 'fb>(
    spi: impl esp_hal::spi::master::Instance + 'd,
    sclk: impl PeripheralOutput<'d>,
    sio0: impl PeripheralInput<'d> + PeripheralOutput<'d>,
    sio1: impl PeripheralInput<'d> + PeripheralOutput<'d>,
    sio2: impl PeripheralInput<'d> + PeripheralOutput<'d>,
    sio3: impl PeripheralInput<'d> + PeripheralOutput<'d>,
    cs: impl PeripheralOutput<'d>,
    dma: impl DmaChannelFor<AnySpi<'d>>,
    reset_pin: Output<'d>,
    fb: &'fb mut [u8],
) -> CO5300<'fb, EspQspi<'d>, Output<'d>> {
    let bus = build_spi(spi, sclk, sio0, sio1, sio2, sio3, cs, dma);
    let mut display = CO5300::new(bus, reset_pin, fb);

    // Hardware reset: short low pulse, then 120 ms settle.
    display.reset_high();
    Timer::after(Duration::from_millis(10)).await;
    display.reset_low();
    Timer::after(Duration::from_millis(10)).await;
    display.reset_high();
    Timer::after(Duration::from_millis(120)).await;

    log::info!("Display: initializing CO5300...");
    display.init().await;
    display.wake().await;
    Timer::after(Duration::from_millis(120)).await; // SLPOUT settle
    display.display_on().await;
    Timer::after(Duration::from_millis(70)).await;
    log::info!("Display: ready ({} FB rows)", display.fb_rows());

    display
}
