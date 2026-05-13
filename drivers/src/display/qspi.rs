//! Trait abstracting the two QSPI write operations the CO5300 needs.
//!
//! The CO5300 uses two distinct SPI opcodes:
//!   0x02 - 1-wire write for configuration/DCS commands
//!   0x32 - 4-wire (quad) write for pixel data
//!
//! Neither maps onto the standard `embedded-hal` `SpiDevice` trait because
//! the chip requires a 24-bit address field to carry the DCS command byte.
//! This thin async trait captures exactly those two operations so the CO5300
//! driver stays portable across any HAL that can implement them.

#[allow(async_fn_in_trait)]
/// Abstraction over the two QSPI write modes used by the CO5300.
///
/// Implementors are responsible for driving CS and any timing constraints
/// between calls.
pub trait QspiWrite {
    type Error;

    /// Send one MIPI DCS command with optional parameters over 1-wire SPI.
    ///
    /// Wire format: opcode=0x02, address=(cmd<<8), data=params
    async fn write_cmd(&mut self, cmd: u8, params: &[u8]) -> Result<(), Self::Error>;

    /// Write pixel bytes over 4-wire (quad) SPI.
    ///
    /// `first` selects the DCS command:
    ///   - `true`  → RAMWR  (0x2C) - start of a new pixel burst
    ///   - `false` → RAMWRC (0x3C) - continuation of the current burst
    ///
    /// Wire format: opcode=0x32, address=(cmd<<8), data=bytes
    ///
    /// Implementations are free to return *before* the underlying DMA
    /// transfer has finished, provided `data` has already been copied
    /// somewhere safe by the time the method returns (so the caller can
    /// mutate `data` immediately). The next call into the bus (including
    /// [`flush_pending`]) must await any such in-flight DMA before
    /// starting its own operation, so the asynchrony is invisible to
    /// callers that don't care.
    ///
    /// [`flush_pending`]: QspiWrite::flush_pending
    async fn write_pixels(&mut self, first: bool, data: &[u8]) -> Result<(), Self::Error>;

    /// Drain any in-flight DMA started by a previous [`write_pixels`]
    /// call so the bus is fully idle on return.
    ///
    /// Call at the end of a render frame to ensure the bus is clean
    /// before other code (sleep transitions, brightness writes, screen
    /// switches) interacts with it. Implementations without pipelined
    /// DMA can leave the default no-op implementation in place.
    ///
    /// [`write_pixels`]: QspiWrite::write_pixels
    async fn flush_pending(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}
