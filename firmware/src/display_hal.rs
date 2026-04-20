//! ESP32-S3 HAL glue for the CO5300 QSPI display.
//!
//! Contains only the hardware-specific parts:
//!   - `build_spi`  - configure the esp-hal QSPI DMA bus
//!   - `EspQspi`    - newtype that implements `drivers::display::QspiWrite`
//!
//! The CO5300 driver itself lives in `drivers::display::co5300`.
//!
//! ## DMA transfers
//!
//! `EspQspi` wraps `SpiDmaBus<Blocking>` which uses GDMA for all transfers.
//! `SpiDmaBus::half_duplex_write` copies the caller's buffer into its own
//! internal DMA TX buffer (internal SRAM) before starting the transfer, so
//! data may come from PSRAM without any extra bounce logic here.
//!
//! The DMA TX buffer is DMA_BUF_SIZE bytes. A larger buffer means fewer SPI
//! transactions per frame (less CS-toggle and DMA-setup overhead).
//! `write_pixels` chunks the framebuffer into pieces that fit the buffer.

use drivers::display::QspiWrite;
use esp_hal::{
    Blocking,
    dma::{DmaChannelFor, DmaRxBuf, DmaTxBuf},
    gpio::interconnect::{PeripheralInput, PeripheralOutput},
    spi::master::{Address, AnySpi, Command, Config, DataMode, Spi, SpiDmaBus},
    spi::Mode,
    time::Rate,
};

pub use drivers::display::{co5300::{FB_BYTES, WIDTH, HEIGHT}, CO5300};

/// DCS commands used inside `write_pixels`.
const RAMWR:  u8 = 0x2C;
const RAMWRC: u8 = 0x3C;

/// SPI instruction opcodes.
const OPCODE_CTRL:  u8 = 0x02; // 1-wire config/command writes
const OPCODE_PIXEL: u8 = 0x32; // Quad 4-wire pixel writes

/// DMA TX buffer capacity in bytes.
/// Larger = fewer SPI transactions per frame = less overhead.
/// 32736 = 8 * 4092 = the maximum single DMA transfer size (MAX_DMA_SIZE in esp-hal).
/// Gives ~13 transactions for a full 410x502 frame vs ~101 at 4092 bytes.
/// RX buffer is kept minimal (4 bytes) since we never read pixel data back.
const DMA_BUF_SIZE: usize = 32736;

/// Wraps the esp-hal DMA SPI bus and provides the `QspiWrite` impl.
///
/// `'d` is the lifetime of the borrowed SPI and DMA peripherals taken from
/// `esp_hal::init`. Construct via [`build_spi`].
pub struct EspQspi<'d> {
    spi: SpiDmaBus<'d, Blocking>,
}

impl<'d> QspiWrite for EspQspi<'d> {
    type Error = esp_hal::spi::Error;

    /// Send one MIPI DCS command over 1-wire SPI (opcode 0x02).
    ///
    /// `params` may be empty - the SPI controller still sends the opcode and
    /// address phase, just with no trailing data bytes.
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
    ///
    /// Data may live in PSRAM. `SpiDmaBus::half_duplex_write` copies each
    /// chunk into its internal DMA TX buffer (internal SRAM) before the
    /// transfer, satisfying the GDMA constraint automatically.
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
/// # Parameters
/// - `spi`  - SPI peripheral (use `p.SPI2`; SPI0/SPI1 are reserved for flash/PSRAM)
/// - `sck`  - clock pin (LCD_SCLK, GPIO11)
/// - `sio0` - data line 0 / MOSI (LCD_SDIO0, GPIO4)
/// - `sio1` - data line 1 (LCD_SDIO1, GPIO5)
/// - `sio2` - data line 2 (LCD_SDIO2, GPIO6)
/// - `sio3` - data line 3 (LCD_SDIO3, GPIO7)
/// - `cs`   - chip select, active low (LCD_CS, GPIO12)
/// - `dma`  - GDMA channel; any of `p.DMA_CH0`/`CH1`/`CH2` not used elsewhere
pub fn build_spi<'d>(
    spi:  impl esp_hal::spi::master::Instance + 'd,
    sck:  impl PeripheralOutput<'d>,
    // esp-hal 1.1 tightened the SIO parameter bound to require both
    // PeripheralInput + PeripheralOutput because QSPI data lines are
    // bidirectional. We still only write from here, but the driver
    // needs the input capability registered.
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

    // 66 MHz: at the CO5300 15ns write spec limit. If this turns out
    // to be unstable in practice we can drop to 40 MHz or push back
    // up to 80 MHz (out of spec but known to work on most panels).
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
