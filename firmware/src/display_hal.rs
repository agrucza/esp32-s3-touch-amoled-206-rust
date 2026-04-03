//! ESP32-S3 HAL glue for the CO5300 QSPI display.
//!
//! Contains only the hardware-specific parts:
//!   - `build_spi`  - configure the esp-hal QSPI DMA bus
//!   - `EspQspi`    - newtype that implements `drivers::display::QspiWrite`
//!
//! The CO5300 driver itself lives in `drivers::display::co5300`.
//!
//! ## Bounce buffer
//!
//! GDMA on ESP32-S3 cannot read from PSRAM directly. `EspQspi` holds a
//! fixed-size bounce buffer in internal SRAM. `write_pixels` copies the
//! incoming data (which may be in PSRAM) into the bounce buffer chunk by
//! chunk before handing each chunk to the DMA engine.

use drivers::display::QspiWrite;
use esp_hal::{
    gpio::interconnect::PeripheralOutput,
    spi::master::{Address, Command, Config, DataMode, Spi},
    spi::Mode,
    time::Rate,
    Blocking,
};

pub use drivers::display::{color, co5300::FB_BYTES, co5300::HEIGHT, co5300::WIDTH, CO5300};

/// DCS commands used inside `write_pixels`.
const RAMWR:  u8 = 0x2C;
const RAMWRC: u8 = 0x3C;

/// SPI instruction opcodes.
const OPCODE_CTRL:  u8 = 0x02; // 1-wire config/command writes
const OPCODE_PIXEL: u8 = 0x32; // Quad 4-wire pixel writes

/// Size of the bounce buffer - must match the ESP32-S3 SPI FIFO (64 bytes = 32 RGB565 pixels).
/// half_duplex_write on blocking SPI (no DMA) cannot transfer more than 64 bytes at once.
const BOUNCE: usize = 64;

/// Wraps the esp-hal blocking SPI bus and provides the `QspiWrite` impl.
///
/// Note: `half_duplex_write` in esp-hal 1.0.0 is synchronous even on async SPI,
/// so we use `Blocking` here. The `QspiWrite` trait is still declared async for
/// future DMA compatibility - the impl just completes without yielding.
pub struct EspQspi<'d> {
    spi:    Spi<'d, Blocking>,
    bounce: [u8; BOUNCE],
}

impl<'d> QspiWrite for EspQspi<'d> {
    type Error = esp_hal::spi::Error;

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
    /// Data may live in PSRAM. This method copies it through the internal
    /// SRAM bounce buffer in BOUNCE-sized chunks before each DMA transfer,
    /// satisfying the GDMA constraint that source memory must be internal SRAM.
    async fn write_pixels(&mut self, first: bool, data: &[u8]) -> Result<(), Self::Error> {
        let mut offset   = 0;
        let mut is_first = first;

        while offset < data.len() {
            let n = (data.len() - offset).min(BOUNCE);

            // Copy from PSRAM (or wherever) into internal SRAM bounce buffer.
            self.bounce[..n].copy_from_slice(&data[offset..offset + n]);

            let dcs = if is_first { RAMWR } else { RAMWRC };
            self.spi.half_duplex_write(
                DataMode::Quad,
                Command::_8Bit(OPCODE_PIXEL as u16, DataMode::Single),
                Address::_24Bit((dcs as u32) << 8, DataMode::Single),
                0,
                &self.bounce[..n],
            )?;

            is_first = false;
            offset  += n;
        }
        Ok(())
    }
}

/// Build and configure the QSPI async SPI bus for the CO5300.
pub fn build_spi<'d>(
    spi:  impl esp_hal::spi::master::Instance + 'd,
    sck:  impl PeripheralOutput<'d>,
    sio0: impl PeripheralOutput<'d>,
    sio1: impl PeripheralOutput<'d>,
    sio2: impl PeripheralOutput<'d>,
    sio3: impl PeripheralOutput<'d>,
    cs:   impl PeripheralOutput<'d>,
) -> EspQspi<'d> {
    let spi = Spi::new(
        spi,
        Config::default()
            .with_frequency(Rate::from_mhz(40))
            .with_mode(Mode::_0),
    )
    .unwrap()
    .with_sck(sck)
    .with_sio0(sio0)
    .with_sio1(sio1)
    .with_sio2(sio2)
    .with_sio3(sio3)
    .with_cs(cs);

    EspQspi { spi, bounce: [0; BOUNCE] }
}
