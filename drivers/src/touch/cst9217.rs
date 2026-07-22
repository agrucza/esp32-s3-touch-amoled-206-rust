//! CST9217 capacitive touch controller driver (Hynitron CST92xx family).
//!
//! Protocol reference: SensorLib `TouchDrvCST92xx.cpp` (the driver the
//! vendor firmware uses on this panel). The chip uses 16-bit register
//! addresses; touch reports live behind a read window at `0xD000` that
//! must be acknowledged with `0xAB` after every read.
//!
//! Unlike the FT3168, the reset line may not be a host GPIO (on the
//! T-Watch Ultra it hangs off the XL9555 expander), so this driver does
//! NOT own a reset pin. The caller performs the hardware reset pulse
//! (low >= 20 ms, then high) before calling [`Cst9217::init`], which
//! waits out the chip's boot window itself.
//!
//! The runtime I2C address is strap-dependent: 0x1A on the T-Watch
//! Ultra. 0x5A is the bootloader address (and would collide with a
//! DRV2605 on the same bus) - firmware-update flows are out of scope
//! here.

use embedded_hal::delay::DelayNs;
use embedded_hal::i2c::I2c as I2cTrait;

use super::TouchEvent;

/// Runtime I2C address on the T-Watch Ultra.
pub const ADDR: u8 = 0x1A;

/// Chip IDs readable from `0xD204` after command-mode entry.
pub const CHIP_ID_CST9217: u16 = 0x9217;
pub const CHIP_ID_CST9220: u16 = 0x9220;

/// Report-window ACK byte.
const ACK: u8 = 0xAB;

/// Reports carry up to 2 fingers: 5 bytes for the first point + flag /
/// count / ACK header bytes interleaved, 5 more per extra point (with a
/// 2-byte skip). 15 bytes covers the whole window.
const REPORT_LEN: usize = 15;

/// 16-bit command/register addresses.
mod reg {
    pub const READ:       u16 = 0xD000; // touch report window
    pub const CMD_MODE:   u16 = 0xD101; // "debug info" / command mode
    pub const SLEEP:      u16 = 0xD105;
    pub const NORMAL:     u16 = 0xD109;
    pub const MODE_LATCH: u16 = 0xD11E; // pre-mode-switch handshake
    pub const CHECKCODE:  u16 = 0xD1FC;
    pub const RESOLUTION: u16 = 0xD1F8;
    pub const CHIP_TYPE:  u16 = 0xD204;
    pub const FW_VERSION: u16 = 0xD208;
    pub const MODE_ECHO:  u16 = 0x0002; // mode readback used by the handshake
}

/// Identity read out of the chip during [`Cst9217::init`].
#[derive(Debug, Clone, Copy)]
pub struct Info {
    pub chip_id:    u16,
    pub project_id: u16,
    pub fw_version: u32,
    /// Touch matrix resolution as configured in the chip's firmware.
    pub res_x: u16,
    pub res_y: u16,
}

/// Consecutive empty-window reads while pressed before a lift-off is
/// synthesized. The chip streams reports at >= 50 Hz while a finger
/// is down (verified on hardware), so consecutive empties only pile
/// up if the real lift-off report was lost (e.g. an I2C error in
/// that window); at the touch task's 8 ms drag cadence this bails
/// out after ~200 ms instead of leaving the state stuck pressed.
const EMPTY_READS_RELEASE: u8 = 25;

/// CST9217 driver. The I2C bus is passed by mutable reference on every
/// call so it can be freely shared with other peripherals on the bus.
pub struct Cst9217 {
    was_pressed: bool,
    /// Consecutive reads that found no new report while pressed.
    empty_reads: u8,
}

impl Cst9217 {
    pub fn new() -> Self {
        Self { was_pressed: false, empty_reads: 0 }
    }

    /// Read and validate the chip identity. Call after the hardware
    /// reset pulse (reset released at least a few ms ago); this waits
    /// out the remaining boot window internally.
    ///
    /// Mirrors the vendor `getAttribute` sequence: enter command mode,
    /// read checkcode / resolution / chip type / firmware version, and
    /// apply the same three validity checks. Like the vendor driver it
    /// does NOT switch back to normal mode afterwards - report reads
    /// work regardless, and this matches the sequence proven on this
    /// hardware.
    pub fn init<I2C, E, D>(&mut self, i2c: &mut I2C, delay: &mut D) -> Result<Info, ()>
    where
        I2C: I2cTrait<Error = E>,
        D: DelayNs,
    {
        // Vendor waits 30 ms after reset release before first I2C.
        delay.delay_ms(30);

        write_cmd(i2c, reg::CMD_MODE)?;
        delay.delay_ms(10);

        let mut buf4 = [0u8; 4];
        read_at(i2c, reg::CHECKCODE, &mut buf4)?;
        let checkcode = u32::from_le_bytes(buf4);

        read_at(i2c, reg::RESOLUTION, &mut buf4)?;
        let res_x = u16::from_le_bytes([buf4[0], buf4[1]]);
        let res_y = u16::from_le_bytes([buf4[2], buf4[3]]);

        read_at(i2c, reg::CHIP_TYPE, &mut buf4)?;
        let project_id = u16::from_le_bytes([buf4[0], buf4[1]]);
        let chip_id = u16::from_le_bytes([buf4[2], buf4[3]]);

        let mut buf8 = [0u8; 8];
        read_at(i2c, reg::FW_VERSION, &mut buf8)?;
        let fw_version = u32::from_le_bytes([buf8[0], buf8[1], buf8[2], buf8[3]]);

        if fw_version == 0xA5A5_A5A5 {
            log::error!("CST9217: no firmware in chip");
            return Err(());
        }
        if (checkcode & 0xFFFF_0000) != 0xCACA_0000 {
            log::error!("CST9217: bad checkcode 0x{:08X}", checkcode);
            return Err(());
        }
        if chip_id != CHIP_ID_CST9217 && chip_id != CHIP_ID_CST9220 {
            log::error!("CST9217: unexpected chip id 0x{:04X}", chip_id);
            return Err(());
        }

        Ok(Info { chip_id, project_id, fw_version, res_x, res_y })
    }

    /// Returns `true` if the last [`read`](Cst9217::read) saw a finger
    /// on screen. Use to keep polling after INT goes quiet so the
    /// lift-off is never missed.
    pub fn is_pressed(&self) -> bool {
        self.was_pressed
    }

    /// Read the current touch state from the report window.
    ///
    /// Call when INT is low, or poll freely. Multi-touch reports carry
    /// up to two fingers; like the FT3168 driver this surfaces only the
    /// first (the watch UI is single-touch).
    pub fn read<I2C, E>(&mut self, i2c: &mut I2C) -> TouchEvent
    where
        I2C: I2cTrait<Error = E>,
    {
        let mut buf = [0u8; REPORT_LEN];
        if read_at(i2c, reg::READ, &mut buf).is_err() {
            return TouchEvent::None;
        }
        // Acknowledge the report so the chip re-arms the window (and
        // releases INT). Vendor sends this before parsing.
        let ack = [
            (reg::READ >> 8) as u8,
            (reg::READ & 0xFF) as u8,
            ACK,
        ];
        if i2c.write(ADDR, &ack).is_err() {
            return TouchEvent::None;
        }

        // Empty window, per the vendor parser: buf[6] must echo the
        // ACK byte; buf[0] equal to it (or zero) means NO NEW REPORT
        // since the last ACK. That is NOT a lift-off - polling faster
        // than the chip's report rate (the touch task's 8 ms drag
        // cadence outpaces it) hits this between reports, and treating
        // it as a release shreds drags into press/release pairs. A
        // real lift-off arrives as a valid report with a non-contact
        // event below. Only synthesize a release if empties persist
        // long enough that the lift-off report must have been lost.
        if buf[0] == ACK || buf[6] != ACK || buf[0] == 0x00 {
            if self.was_pressed {
                self.empty_reads += 1;
                if self.empty_reads >= EMPTY_READS_RELEASE {
                    self.was_pressed = false;
                    self.empty_reads = 0;
                    return TouchEvent::Released;
                }
            }
            return TouchEvent::None;
        }
        self.empty_reads = 0;

        let count = buf[5] & 0x7F;

        // First point: id/event nibble byte, x/y high bytes, packed
        // low nibbles. Event 0x06 = finger in contact; a report with
        // any other event (0x00 = lift-off), no points, or the
        // cover-screen gesture flag (buf[4] bit 7) ends the touch.
        let event = buf[0] & 0x0F;
        let x = ((buf[1] as u16) << 4) | ((buf[3] as u16) >> 4);
        let y = ((buf[2] as u16) << 4) | ((buf[3] as u16) & 0x0F);

        if (buf[4] & 0x80) != 0 || count == 0 || count > 2 || event != 0x06 {
            if self.was_pressed {
                self.was_pressed = false;
                return TouchEvent::Released;
            }
            return TouchEvent::None;
        }

        self.was_pressed = true;
        TouchEvent::Pressed { x, y }
    }

    /// Put the controller into deep sleep.
    ///
    /// Wake requires a hardware reset pulse (there is no I2C wake), so
    /// the caller must re-run the reset + [`init`](Cst9217::init)
    /// sequence afterwards.
    pub fn sleep<I2C, E, D>(&mut self, i2c: &mut I2C, delay: &mut D) -> Result<(), ()>
    where
        I2C: I2cTrait<Error = E>,
        D: DelayNs,
    {
        self.enter_command_mode(i2c, delay)?;
        write_cmd(i2c, reg::SLEEP)
    }

    /// Switch back to normal reporting mode (after a debug-mode
    /// excursion; not needed after init).
    pub fn set_normal_mode<I2C, E, D>(&mut self, i2c: &mut I2C, delay: &mut D) -> Result<(), ()>
    where
        I2C: I2cTrait<Error = E>,
        D: DelayNs,
    {
        self.enter_command_mode(i2c, delay)?;
        write_cmd(i2c, reg::NORMAL)?;
        let mut echo = [0u8; 2];
        read_at(i2c, reg::MODE_ECHO, &mut echo)?;
        if echo[1] != (reg::NORMAL & 0xFF) as u8 {
            return Err(());
        }
        delay.delay_ms(10);
        Ok(())
    }

    /// The vendor pre-mode-switch handshake: latch `0xD11E` (sent
    /// twice) until the mode-echo register reads it back.
    fn enter_command_mode<I2C, E, D>(&mut self, i2c: &mut I2C, delay: &mut D) -> Result<(), ()>
    where
        I2C: I2cTrait<Error = E>,
        D: DelayNs,
    {
        for _ in 0..3 {
            if write_cmd(i2c, reg::MODE_LATCH).is_err()
                || write_cmd(i2c, reg::MODE_LATCH).is_err()
            {
                delay.delay_ms(200);
                continue;
            }
            let mut echo = [0u8; 4];
            if read_at(i2c, reg::MODE_ECHO, &mut echo).is_err() {
                delay.delay_ms(200);
                continue;
            }
            if echo[1] == (reg::MODE_LATCH & 0xFF) as u8 {
                return Ok(());
            }
        }
        Err(())
    }
}

impl Default for Cst9217 {
    fn default() -> Self {
        Self::new()
    }
}

// ---- private helpers --------------------------------------------------------

/// Write a bare 16-bit command (register address, no payload).
fn write_cmd<I2C, E>(i2c: &mut I2C, r: u16) -> Result<(), ()>
where
    I2C: I2cTrait<Error = E>,
{
    i2c.write(ADDR, &[(r >> 8) as u8, (r & 0xFF) as u8]).map_err(|_| ())
}

/// Read `buf.len()` bytes starting at 16-bit register address `r`.
fn read_at<I2C, E>(i2c: &mut I2C, r: u16, buf: &mut [u8]) -> Result<(), ()>
where
    I2C: I2cTrait<Error = E>,
{
    i2c.write_read(ADDR, &[(r >> 8) as u8, (r & 0xFF) as u8], buf)
        .map_err(|_| ())
}
