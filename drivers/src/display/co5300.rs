//! CO5300 QSPI AMOLED display driver - HAL-agnostic, framebuffer-based.
//!
//! ## Architecture
//!
//! The driver holds a mutable reference to a framebuffer slice provided by the
//! caller. All drawing operations (fill, blit, DrawTarget) are synchronous RAM
//! writes into the framebuffer - no bus I/O, no waiting. When the scene is
//! ready, call `flush()` which sends the entire framebuffer to the display over
//! the QSPI bus (using DMA or whatever transport the `QspiWrite` implementor
//! provides).
//!
//! On ESP32-S3R8 the framebuffer should be placed in PSRAM:
//!
//! ```rust
//! #[link_section = ".ext_ram.bss"]
//! static mut FRAMEBUFFER: [u8; co5300::FB_BYTES] = [0; co5300::FB_BYTES];
//!
//! let display = CO5300::new(bus, reset, unsafe { &mut FRAMEBUFFER });
//! ```
//!
//! GDMA cannot DMA from PSRAM directly; the `QspiWrite` implementor is
//! responsible for bouncing data through an internal-SRAM buffer before
//! handing it to the DMA engine (see `EspQspi` in the firmware crate).
//!
//! ## Wire format (CO5300 datasheet section 5.2.2)
//!
//! - Control: opcode=0x02, addr=(CMD<<8), data on 1 wire
//! - Pixels:  opcode=0x32, addr=(CMD<<8), data on 4 wires
//!
//! Do NOT send 0x38 (Enter Quad SPI) - it breaks the mixed opcode approach.

use embedded_hal::digital::OutputPin;
use embedded_graphics_core::{
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Size},
    pixelcolor::{Rgb565, RgbColor},
    primitives::Rectangle,
    Pixel,
};

use super::qspi::QspiWrite;

/// Display resolution (panel H0198S005AMT005_V0_195).
pub const WIDTH:  u16 = 410;
pub const HEIGHT: u16 = 502;

/// Framebuffer size in bytes (RGB565, 2 bytes per pixel).
pub const FB_BYTES: usize = WIDTH as usize * HEIGHT as usize * 2;

/// Column / row offsets applied by this panel inside the CO5300 frame buffer.
const COL_OFFSET: u16 = 22;
const ROW_OFFSET: u16 = 0;

/// MIPI DCS command bytes.
pub mod cmd {
    pub const SLPIN:      u8 = 0x10;
    pub const SLPOUT:     u8 = 0x11;
    pub const DISPOFF:    u8 = 0x28;
    pub const DISPON:     u8 = 0x29;
    pub const CASET:      u8 = 0x2A;
    pub const RASET:      u8 = 0x2B;
    pub const RAMWR:      u8 = 0x2C;
    pub const RAMWRC:     u8 = 0x3C;
    pub const MADCTL:     u8 = 0x36;
    pub const PIXFMT:     u8 = 0x3A;
    pub const BRIGHTNESS: u8 = 0x51;
}

/// Named RGB565 colour constants.
pub mod color {
    pub const BLACK: u16 = 0x0000;
    pub const WHITE: u16 = 0xFFFF;
    pub const RED:   u16 = 0xF800;
    pub const GREEN: u16 = 0x07E0;
    pub const BLUE:  u16 = 0x001F;
}

/// Display orientation / mirroring sent as the MADCTL parameter (0x36).
///
/// The CO5300 MADCTL only implements MY (bit7), MX (bit6), and RGB (bit3).
/// There is no MV bit - true 90/270 degree rotation requires software transposition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Rotation {
    #[default]
    Normal  = 0x00,   // MY=0 MX=0
    MirrorX = 0x40,   // MX=1
    MirrorY = 0x80,   // MY=1
    Deg180  = 0xC0,   // MY=1 MX=1
}

// ---- Driver struct ----------------------------------------------------------

/// CO5300 driver. Generic over the QSPI bus `B` and reset pin `RST`.
///
/// `'fb` is the lifetime of the caller-provided framebuffer slice.
pub struct CO5300<'fb, B, RST> {
    bus:         B,
    reset:       RST,
    framebuffer: &'fb mut [u8],
}

// ---- Init / power / config - all async (bus I/O) ----------------------------

impl<'fb, B: QspiWrite, RST: OutputPin> CO5300<'fb, B, RST> {
    /// Create a new driver.
    ///
    /// `framebuffer` must be exactly [`FB_BYTES`] bytes long and should be
    /// placed in PSRAM on ESP32-S3 via `#[link_section = ".ext_ram.bss"]`.
    pub fn new(bus: B, reset: RST, framebuffer: &'fb mut [u8]) -> Self {
        assert_eq!(framebuffer.len(), FB_BYTES, "framebuffer must be WIDTH*HEIGHT*2 bytes");
        Self { bus, reset, framebuffer }
    }

    // ---- Reset helpers (caller provides delays via Timer::after) --------

    pub fn reset_high(&mut self) { self.reset.set_high().ok(); }
    pub fn reset_low(&mut self)  { self.reset.set_low().ok();  }

    // ---- Init sequence --------------------------------------------------

    /// Send configuration commands. Call order:
    ///   1. hardware reset (RST low then high, 120 ms settle)
    ///   2. `init()`
    ///   3. `wake()` + 120 ms
    ///   4. `display_on()` + 70 ms
    pub async fn init(&mut self) {
        self.write_cmd(0xC4,            &[0x80]).await; // enable QSPI opcode 0x32
        self.write_cmd(cmd::PIXFMT,     &[0x55]).await; // RGB565 (16 bpp)
        self.write_cmd(0x35,            &[0x00]).await; // tearing effect on, mode 0
        self.write_cmd(0x53,            &[0x20]).await; // BC_EN=1
        self.write_cmd(cmd::BRIGHTNESS, &[80]  ).await; // ~31%
        self.write_cmd(0x63,            &[0xFF]).await; // HBM max
    }

    // ---- Power / backlight ----------------------------------------------

    /// Enter sleep mode.
    ///
    /// Timing (datasheet section 5.6.2 / 7.5.11):
    ///   - Wait >= 5 ms after this command before any next command.
    ///   - Wait >= 120 ms after SLPOUT before issuing SLPIN.
    pub async fn sleep(&mut self) {
        self.write_cmd(cmd::SLPIN, &[]).await;
    }

    /// Exit sleep mode.
    ///
    /// Timing (datasheet section 5.6.1 / 7.5.12):
    ///   - Wait >= 5 ms after this command before any next command.
    ///   - Wait >= 120 ms after SLPIN before issuing SLPOUT.
    ///   - Call `flush()` only after the 120 ms settle has elapsed.
    pub async fn wake(&mut self) {
        self.write_cmd(cmd::SLPOUT, &[]).await;
    }

    /// Turn pixels on (display visible).
    pub async fn display_on(&mut self) {
        self.write_cmd(cmd::DISPON, &[]).await;
    }

    /// Turn pixels off (panel blanked, backlight on).
    pub async fn display_off(&mut self) {
        self.write_cmd(cmd::DISPOFF, &[]).await;
    }

    /// Set display brightness (0 = off, 255 = maximum).
    pub async fn set_brightness(&mut self, brightness: u8) {
        self.write_cmd(cmd::BRIGHTNESS, &[brightness]).await;
    }

    /// Set display orientation. See [`Rotation`] for available modes.
    pub async fn set_rotation(&mut self, rotation: Rotation) {
        self.write_cmd(cmd::MADCTL, &[rotation as u8]).await;
    }

    /// Send one raw MIPI DCS command with optional parameters.
    pub async fn write_cmd(&mut self, command: u8, params: &[u8]) {
        self.bus.write_cmd(command, params).await.ok();
    }

    // ---- Flush ----------------------------------------------------------

    /// Send the entire framebuffer to the display.
    ///
    /// This is the only async drawing operation. All drawing methods write
    /// to the in-RAM framebuffer synchronously; call `flush()` when the
    /// scene is complete to push it to the panel in one DMA transfer.
    ///
    /// The `QspiWrite` implementor is responsible for bouncing data through
    /// internal SRAM if the framebuffer resides in PSRAM.
    pub async fn flush(&mut self) {
        self.set_addr_window(0, 0, WIDTH, HEIGHT).await;
        self.bus.write_pixels(true, self.framebuffer).await.ok();
    }

    // ---- Private helpers ------------------------------------------------

    async fn set_addr_window(&mut self, x: u16, y: u16, w: u16, h: u16) {
        let x0 = x + COL_OFFSET;
        let x1 = x + w - 1 + COL_OFFSET;
        let y0 = y + ROW_OFFSET;
        let y1 = y + h - 1 + ROW_OFFSET;
        self.bus.write_cmd(cmd::CASET, &[
            (x0 >> 8) as u8, (x0 & 0xFF) as u8,
            (x1 >> 8) as u8, (x1 & 0xFF) as u8,
        ]).await.ok();
        self.bus.write_cmd(cmd::RASET, &[
            (y0 >> 8) as u8, (y0 & 0xFF) as u8,
            (y1 >> 8) as u8, (y1 & 0xFF) as u8,
        ]).await.ok();
    }
}

// ---- Sync drawing API (framebuffer writes only) -----------------------------

impl<'fb, B, RST> CO5300<'fb, B, RST> {
    /// Fill a rectangle with a single RGB565 colour.
    pub fn fill_solid(&mut self, x: u16, y: u16, w: u16, h: u16, color: u16) {
        let hi = (color >> 8) as u8;
        let lo = (color & 0xFF) as u8;
        for row in y..y + h {
            let row_base = row as usize * WIDTH as usize;
            for col in x..x + w {
                let idx = (row_base + col as usize) * 2;
                self.framebuffer[idx]     = hi;
                self.framebuffer[idx + 1] = lo;
            }
        }
    }

    /// Blit a rectangle of RGB565 pixels from a slice.
    ///
    /// `pixels` must contain exactly `w * h` values.
    pub fn draw_raw(&mut self, x: u16, y: u16, w: u16, h: u16, pixels: &[u16]) {
        let mut src = 0usize;
        for row in y..y + h {
            let row_base = row as usize * WIDTH as usize;
            for col in x..x + w {
                if src >= pixels.len() { return; }
                let px  = pixels[src];
                let idx = (row_base + col as usize) * 2;
                self.framebuffer[idx]     = (px >> 8) as u8;
                self.framebuffer[idx + 1] = (px & 0xFF) as u8;
                src += 1;
            }
        }
    }

    /// Write a single pixel to the framebuffer.
    pub fn draw_pixel(&mut self, x: u16, y: u16, color: u16) {
        if x < WIDTH && y < HEIGHT {
            let idx = (y as usize * WIDTH as usize + x as usize) * 2;
            self.framebuffer[idx]     = (color >> 8) as u8;
            self.framebuffer[idx + 1] = (color & 0xFF) as u8;
        }
    }
}

// ---- embedded-graphics DrawTarget ------------------------------------------

impl<'fb, B, RST> OriginDimensions for CO5300<'fb, B, RST> {
    fn size(&self) -> Size {
        Size::new(WIDTH as u32, HEIGHT as u32)
    }
}

impl<'fb, B, RST> DrawTarget for CO5300<'fb, B, RST> {
    type Color = Rgb565;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Rgb565>>,
    {
        for Pixel(pt, color) in pixels {
            // Compare in i32 so values outside the u16 range can't wrap
            // through `as u16` into an apparently-valid coordinate.
            if pt.x >= 0
                && pt.y >= 0
                && pt.x < WIDTH  as i32
                && pt.y < HEIGHT as i32
            {
                let raw = ((color.r() as u16) << 11)
                    | ((color.g() as u16) << 5)
                    | (color.b() as u16);
                let idx = (pt.y as usize * WIDTH as usize + pt.x as usize) * 2;
                self.framebuffer[idx]     = (raw >> 8) as u8;
                self.framebuffer[idx + 1] = (raw & 0xFF) as u8;
            }
        }
        Ok(())
    }

    fn fill_contiguous<I>(
        &mut self,
        area: &Rectangle,
        colors: I,
    ) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Rgb565>,
    {
        // Work entirely in signed i32 so we can reason about areas that
        // straddle the framebuffer edges (e.g. a rectangle with negative
        // y origin). Casting an i32 < 0 to u16 wraps to a huge value
        // and silently breaks any subsequent .min() clipping.
        let ax0 = area.top_left.x;
        let ay0 = area.top_left.y;
        let ax1 = ax0 + area.size.width  as i32; // exclusive
        let ay1 = ay0 + area.size.height as i32; // exclusive

        // Completely outside the framebuffer - nothing to draw and the
        // source iterator can simply be dropped.
        if ax1 <= 0 || ay1 <= 0 || ax0 >= WIDTH as i32 || ay0 >= HEIGHT as i32 {
            return Ok(());
        }

        // Clip to framebuffer bounds, exclusive on the right/bottom.
        let cx0 = ax0.max(0);
        let cy0 = ay0.max(0);
        let cx1 = ax1.min(WIDTH  as i32);
        let cy1 = ay1.min(HEIGHT as i32);

        // Count of source pixels to skip on each side of the clip. These
        // are always >= 0 because of the .max(0) / .min(...) clamps.
        let skip_left  = (cx0 - ax0) as usize;
        let skip_right = (ax1 - cx1) as usize;
        let skip_top   = (cy0 - ay0) as usize;
        let src_row_w  = area.size.width as usize;

        let mut colors = colors.into_iter();

        // Advance past any entirely-clipped top rows so the first drawn
        // row reads the correct source pixels.
        for _ in 0..(skip_top * src_row_w) {
            if colors.next().is_none() { return Ok(()); }
        }

        for row in cy0..cy1 {
            // Skip pixels clipped on the left edge.
            for _ in 0..skip_left {
                if colors.next().is_none() { return Ok(()); }
            }

            let row_base = row as usize * WIDTH as usize;
            for col in cx0..cx1 {
                match colors.next() {
                    None => return Ok(()),
                    Some(color) => {
                        let raw = ((color.r() as u16) << 11)
                            | ((color.g() as u16) << 5)
                            | (color.b() as u16);
                        let idx = (row_base + col as usize) * 2;
                        self.framebuffer[idx]     = (raw >> 8) as u8;
                        self.framebuffer[idx + 1] = (raw & 0xFF) as u8;
                    }
                }
            }

            // Skip pixels clipped on the right edge.
            for _ in 0..skip_right {
                if colors.next().is_none() { return Ok(()); }
            }
        }
        Ok(())
    }

    fn clear(&mut self, color: Rgb565) -> Result<(), Self::Error> {
        let raw = ((color.r() as u16) << 11)
            | ((color.g() as u16) << 5)
            | (color.b() as u16);
        let hi = (raw >> 8) as u8;
        let lo = (raw & 0xFF) as u8;
        for chunk in self.framebuffer.chunks_exact_mut(2) {
            chunk[0] = hi;
            chunk[1] = lo;
        }
        Ok(())
    }
}
