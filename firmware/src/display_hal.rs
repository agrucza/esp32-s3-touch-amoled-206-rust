//! ESP32-S3 HAL glue for the CO5300 QSPI display.
//!
//! This module contains only the hardware-specific parts:
//!   - `build_spi`    — configure the esp-hal QSPI bus
//!   - `EspQspi`      — newtype wrapper that implements `drivers::display::QspiWrite`
//!
//! The CO5300 driver itself (init, drawing, DrawTarget) lives in `drivers::display::co5300`.

use drivers::display::QspiWrite;
use esp_hal::{
    gpio::interconnect::PeripheralOutput,
    spi::master::{Address, Command, Config, DataMode, Spi},
    spi::Mode,
    time::Rate,
    Blocking,
};

// Re-export the DCS command bytes and colour constants so main.rs doesn't
// need a separate import path.
pub use drivers::display::{cmd, color, co5300::WIDTH, co5300::HEIGHT, CO5300, Rotation};

/// DCS commands needed by `QspiWrite::write_pixels`.
const RAMWR:  u8 = 0x2C;
const RAMWRC: u8 = 0x3C;

/// SPI instruction opcodes (not DCS commands).
const OPCODE_CTRL:  u8 = 0x02; // 1-wire write (config commands)
const OPCODE_PIXEL: u8 = 0x32; // Quad 4-wire pixel write

/// Newtype wrapping the esp-hal blocking SPI bus.
pub struct EspQspi<'d>(pub Spi<'d, Blocking>);

impl<'d> QspiWrite for EspQspi<'d> {
    type Error = esp_hal::spi::Error;

    fn write_cmd(&mut self, cmd: u8, params: &[u8]) -> Result<(), Self::Error> {
        self.0.half_duplex_write(
            DataMode::Single,
            Command::_8Bit(OPCODE_CTRL as u16, DataMode::Single),
            Address::_24Bit((cmd as u32) << 8, DataMode::Single),
            0,
            params,
        )
    }

    fn write_pixels(&mut self, first: bool, data: &[u8]) -> Result<(), Self::Error> {
        let dcs = if first { RAMWR } else { RAMWRC };
        self.0.half_duplex_write(
            DataMode::Quad,
            Command::_8Bit(OPCODE_PIXEL as u16, DataMode::Single),
            Address::_24Bit((dcs as u32) << 8, DataMode::Single),
            0,
            data,
        )
    }
}

/// Build and configure the QSPI bus for the CO5300.
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

    EspQspi(spi)
}
